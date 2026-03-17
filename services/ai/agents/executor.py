"""Non-interactive agent executor — mirrors the chat loop but without streaming/approval."""

import asyncio
import json
import logging
from datetime import datetime, timezone
from typing import cast

from anthropic.types import (
    MessageParam,
    TextBlockParam,
    ToolUseBlockParam,
    ToolResultBlockParam,
)

from config import (
    AGENT_MAX_ITERATIONS,
    DEFAULT_MAX_TOKENS,
    DEFAULT_TEMPERATURE,
    DEFAULT_TOP_P,
    CONNECTOR_MANAGER_URL,
    SANDBOX_URL,
)
from db.documents import DocumentsRepository
from db.models import Source
from providers import LLMProvider
from prompts import build_agent_system_prompt
from services.compaction import ConversationCompactor
from state import AppState
from tools import (
    ToolRegistry,
    ToolContext,
    SearchToolHandler,
    ConnectorToolHandler,
    DocumentToolHandler,
)
from tools.sandbox_handler import SandboxToolHandler

from .models import Agent, AgentRun
from .repository import AgentRunRepository

logger = logging.getLogger(__name__)


def _resolve_llm_provider(state: AppState, agent: Agent) -> LLMProvider:
    """Resolve which LLM provider to use for an agent."""
    models = state.models
    if not models:
        raise RuntimeError("No models configured")

    if agent.model_id and agent.model_id in models:
        return models[agent.model_id]

    if state.default_model_id and state.default_model_id in models:
        return models[state.default_model_id]

    return next(iter(models.values()))


async def _fetch_sources() -> list[Source] | None:
    """Fetch all sources from the connector manager."""
    if not CONNECTOR_MANAGER_URL:
        return None
    import httpx

    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            resp = await client.get(f"{CONNECTOR_MANAGER_URL.rstrip('/')}/sources")
            resp.raise_for_status()
            return [Source.from_row(s) for s in resp.json()]
    except Exception as e:
        logger.warning(f"Failed to fetch sources: {e}")
        return None


def _build_source_filter(agent: Agent) -> dict[str, list[str]] | None:
    """Build source_filter dict from agent.allowed_sources."""
    if not agent.allowed_sources:
        return None
    return {
        entry["source_id"]: entry.get("modes", ["read"])
        for entry in agent.allowed_sources
    }


async def _build_agent_registry(
    app_state: AppState,
    agent: Agent,
    sources: list[Source] | None,
) -> tuple[ToolRegistry, list | None]:
    """Build a ToolRegistry configured for the agent's permissions."""
    registry = ToolRegistry()

    source_filter = _build_source_filter(agent) if agent.agent_type == "user" else None
    action_whitelist = agent.allowed_actions if agent.agent_type == "org" else None

    connector_actions = None

    if CONNECTOR_MANAGER_URL:
        connector_handler = ConnectorToolHandler(
            connector_manager_url=CONNECTOR_MANAGER_URL,
            user_id=agent.user_id,
            redis_client=app_state.redis_client,
            prefetched_sources=sources,
            source_filter=source_filter,
            action_whitelist=action_whitelist,
        )
        await connector_handler._ensure_initialized()
        registry.register(connector_handler)

        if connector_handler._actions:
            from tools.connector_handler import ConnectorAction

            connector_actions = list(connector_handler._actions.values())

    # Search tool — always registered
    search_operators = None
    if CONNECTOR_MANAGER_URL and connector_actions:
        search_operators = connector_handler.search_operators

    registry.register(
        SearchToolHandler(
            searcher_tool=app_state.searcher_tool,
            search_operators=search_operators,
        )
    )

    # Document handler
    content_storage = app_state.content_storage
    if content_storage or CONNECTOR_MANAGER_URL:
        registry.register(
            DocumentToolHandler(
                content_storage=content_storage,
                documents_repo=DocumentsRepository(),
                sandbox_url=SANDBOX_URL or None,
                connector_manager_url=CONNECTOR_MANAGER_URL or None,
            )
        )

    # Sandbox tools — only for user agents
    if SANDBOX_URL and agent.agent_type == "user":
        registry.register(SandboxToolHandler(sandbox_url=SANDBOX_URL))

    return registry, connector_actions


async def execute_agent(
    agent: Agent,
    app_state: AppState,
    status_queue: asyncio.Queue | None = None,
) -> AgentRun:
    """Execute a background agent run (non-interactive).

    Args:
        agent: The agent configuration
        app_state: Application state with models, searcher, etc.
        status_queue: Optional queue for live status events
    """
    run_repo = AgentRunRepository()
    run = await run_repo.create_run(agent.id)

    now = datetime.now(timezone.utc)
    run = await run_repo.update_run(run.id, status="running", started_at=now)

    async def emit_status(message: str):
        if status_queue:
            await status_queue.put({"type": "status", "message": message})

    try:
        await emit_status("Initializing...")

        llm_provider = _resolve_llm_provider(app_state, agent)
        sources = await _fetch_sources()

        registry, connector_actions = await _build_agent_registry(
            app_state, agent, sources
        )
        all_tools = registry.get_all_tools()

        # Build system prompt
        active_sources = [
            s for s in (sources or []) if s.is_active and not s.is_deleted
        ]
        system_prompt = build_agent_system_prompt(
            agent, active_sources, connector_actions
        )

        # Initialize conversation with a single trigger message
        conversation_messages: list[MessageParam] = [
            MessageParam(role="user", content="Execute your scheduled task now.")
        ]
        execution_log: list[dict] = list(conversation_messages)

        # Org agents search all data (no user-scoping); personal agents are scoped to owner
        context = ToolContext(
            chat_id=f"agent-run:{run.id}",
            user_id=None if agent.agent_type == "org" else agent.user_id,
        )

        # Compaction support
        compactor = ConversationCompactor(
            llm_provider=llm_provider,
            redis_client=app_state.redis_client,
        )

        for iteration in range(AGENT_MAX_ITERATIONS):
            logger.info(f"Agent {agent.id} run {run.id}: iteration {iteration + 1}")

            # Check if compaction is needed
            if compactor.needs_compaction(conversation_messages, all_tools):
                logger.info(f"Compacting conversation for agent run {run.id}")
                conversation_messages = await compactor.compact_conversation(
                    f"agent-run:{run.id}", conversation_messages
                )

            # Call LLM (non-streaming — collect full response)
            content_blocks: list[TextBlockParam | ToolUseBlockParam] = []

            stream = llm_provider.stream_response(
                prompt="",
                messages=conversation_messages,
                tools=all_tools,
                max_tokens=DEFAULT_MAX_TOKENS,
                temperature=DEFAULT_TEMPERATURE,
                top_p=DEFAULT_TOP_P,
                system_prompt=system_prompt,
            )

            async for event in stream:
                if event.type == "content_block_start":
                    if event.content_block.type == "text":
                        content_blocks.append(
                            TextBlockParam(type="text", text=event.content_block.text)
                        )
                    elif event.content_block.type == "tool_use":
                        content_blocks.append(
                            ToolUseBlockParam(
                                type="tool_use",
                                id=event.content_block.id,
                                name=event.content_block.name,
                                input="",
                            )
                        )
                elif event.type == "content_block_delta":
                    if event.delta.type == "text_delta":
                        if event.index < len(content_blocks):
                            text_block = cast(
                                TextBlockParam, content_blocks[event.index]
                            )
                            text_block["text"] += event.delta.text
                    elif event.delta.type == "input_json_delta":
                        if event.index < len(content_blocks):
                            tool_block = cast(
                                ToolUseBlockParam, content_blocks[event.index]
                            )
                            tool_block["input"] = (
                                cast(str, tool_block["input"])
                                + event.delta.partial_json
                            )
                elif event.type == "message_stop":
                    break

            # Parse tool call inputs
            tool_calls = [b for b in content_blocks if b["type"] == "tool_use"]
            for tool_call in tool_calls:
                try:
                    tool_call["input"] = json.loads(cast(str, tool_call["input"]))
                except json.JSONDecodeError:
                    tool_call["input"] = {}

            assistant_message = MessageParam(role="assistant", content=content_blocks)
            conversation_messages.append(assistant_message)
            execution_log.append(assistant_message)

            # No tool calls — done
            if not tool_calls:
                logger.info(f"Agent {agent.id} run {run.id}: no tool calls, completing")
                break

            # Execute tool calls — no approval needed
            tool_results: list[ToolResultBlockParam] = []
            for tool_call in tool_calls:
                tool_name = tool_call["name"]
                await emit_status(f"Executing: {tool_name}")

                result = await registry.execute(tool_name, tool_call["input"], context)
                tool_results.append(
                    ToolResultBlockParam(
                        type="tool_result",
                        tool_use_id=tool_call["id"],
                        content=result.content,
                        is_error=result.is_error,
                    )
                )

            tool_result_message = MessageParam(role="user", content=tool_results)
            conversation_messages.append(tool_result_message)
            execution_log.append(tool_result_message)

        # Generate summary using one final LLM turn
        await emit_status("Generating summary...")
        summary_prompt_message = MessageParam(
            role="user",
            content=(
                "Provide a brief summary (2-3 sentences) of what you just did and the outcomes. "
                "Be factual and concise."
            ),
        )
        conversation_messages.append(summary_prompt_message)

        summary_blocks: list = []
        summary_stream = llm_provider.stream_response(
            prompt="",
            messages=conversation_messages,
            tools=[],
            max_tokens=500,
            temperature=0.3,
            system_prompt=system_prompt,
        )
        async for event in summary_stream:
            if (
                event.type == "content_block_start"
                and event.content_block.type == "text"
            ):
                summary_blocks.append(event.content_block.text)
            elif (
                event.type == "content_block_delta" and event.delta.type == "text_delta"
            ):
                summary_blocks.append(event.delta.text)
            elif event.type == "message_stop":
                break

        summary = "".join(summary_blocks).strip()

        completed_at = datetime.now(timezone.utc)
        run = await run_repo.update_run(
            run.id,
            status="completed",
            completed_at=completed_at,
            execution_log=execution_log,
            summary=summary,
        )

        if status_queue:
            await status_queue.put({"type": "completed", "summary": summary})

        logger.info(f"Agent {agent.id} run {run.id} completed successfully")
        return run

    except Exception as e:
        logger.error(f"Agent {agent.id} run {run.id} failed: {e}", exc_info=True)

        completed_at = datetime.now(timezone.utc)
        run = await run_repo.update_run(
            run.id,
            status="failed",
            completed_at=completed_at,
            execution_log=execution_log if "execution_log" in locals() else [],
            error_message=str(e),
        )

        if status_queue:
            await status_queue.put({"type": "failed", "error": str(e)})

        return run
