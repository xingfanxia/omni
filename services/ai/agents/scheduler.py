"""Background scheduler that polls for due agents and executes them."""

import asyncio
import logging
from datetime import datetime, timezone

from config import AGENT_SCHEDULER_POLL_INTERVAL, AGENT_MAX_CONCURRENT_RUNS
from state import AppState
from .executor import execute_agent
from .repository import AgentRepository

logger = logging.getLogger(__name__)


async def run_agent_scheduler(app_state: AppState):
    """Long-running scheduler loop. Launched as asyncio.create_task from main.py startup."""
    logger.info(
        f"Agent scheduler started (poll_interval={AGENT_SCHEDULER_POLL_INTERVAL}s, "
        f"max_concurrent={AGENT_MAX_CONCURRENT_RUNS})"
    )

    semaphore = asyncio.Semaphore(AGENT_MAX_CONCURRENT_RUNS)
    agent_repo = AgentRepository()

    while True:
        try:
            now = datetime.now(timezone.utc)
            due_agents = await agent_repo.find_due_agents(now)

            if due_agents:
                logger.info(f"Found {len(due_agents)} due agent(s)")

            for agent in due_agents:
                # Spawn bounded task
                asyncio.create_task(_run_with_semaphore(semaphore, agent, app_state))

        except Exception as e:
            logger.error(f"Agent scheduler tick failed: {e}", exc_info=True)

        await asyncio.sleep(AGENT_SCHEDULER_POLL_INTERVAL)


async def _run_with_semaphore(semaphore: asyncio.Semaphore, agent, app_state: AppState):
    """Execute an agent run with concurrency limiting."""
    async with semaphore:
        try:
            logger.info(f"Starting scheduled run for agent {agent.id} ({agent.name})")

            # Create a status queue for this run
            status_queue = asyncio.Queue()
            await execute_agent(agent, app_state, status_queue=status_queue)

        except Exception as e:
            logger.error(
                f"Scheduled run for agent {agent.id} failed: {e}", exc_info=True
            )
