"""Telegram connector for Omni."""

import logging
from datetime import datetime, timezone
from typing import Any

from omni_connector import Connector, SyncContext

from .client import AuthenticationError, TelegramClient, TelegramError
from .config import CHECKPOINT_INTERVAL, MAX_CONTENT_LENGTH
from .mappers import (
    generate_chat_content,
    generate_message_content,
    map_chat_to_document,
    map_message_to_document,
)

logger = logging.getLogger(__name__)


class TelegramConnector(Connector):
    """Telegram connector for Omni.

    Indexes messages from Telegram chats, groups, and channels
    accessible to a bot. Uses the Bot API (getUpdates) for message
    history. The bot must be added to the chats it should index.
    """

    @property
    def name(self) -> str:
        return "telegram"

    @property
    def version(self) -> str:
        return "1.0.0"

    @property
    def source_types(self) -> list[str]:
        return ["telegram"]

    @property
    def description(self) -> str:
        return "Connect to Telegram chats, groups, and channels"

    @property
    def sync_modes(self) -> list[str]:
        return ["full", "incremental"]

    async def sync(
        self,
        source_config: dict[str, Any],
        credentials: dict[str, Any],
        state: dict[str, Any] | None,
        ctx: SyncContext,
    ) -> None:
        """Execute a sync operation."""
        token = credentials.get("token")
        if not token:
            await ctx.fail("Missing 'token' in credentials (Telegram Bot API token)")
            return

        client = TelegramClient(token=token)

        try:
            bot_user = await client.get_me()
            bot_username = bot_user.get("username", "bot")
            logger.info("Authenticated as @%s", bot_username)
        except AuthenticationError as e:
            await ctx.fail(f"Authentication failed: {e}")
            return
        except TelegramError as e:
            await ctx.fail(f"Failed to connect: {e}")
            return

        state = state or {}
        chat_ids = source_config.get("chat_ids", [])

        try:
            if not chat_ids:
                # No explicit chat list — use getUpdates to discover chats
                await self._sync_via_updates(client, bot_username, state, ctx)
            else:
                # Sync specific chats
                await self._sync_specific_chats(
                    client, bot_username, chat_ids, state, ctx
                )
        except AuthenticationError as e:
            await ctx.fail(f"Authentication failed: {e}")
        except TelegramError as e:
            await ctx.fail(f"Sync failed: {e}")
        finally:
            await client.close()

    async def _sync_via_updates(
        self,
        client: TelegramClient,
        bot_username: str,
        state: dict[str, Any],
        ctx: SyncContext,
    ) -> None:
        """Sync messages using getUpdates (bot must have received messages)."""
        last_update_id = state.get("last_update_id")
        offset = (last_update_id + 1) if last_update_id else None

        docs_emitted = 0
        seen_chats: dict[int, dict[str, Any]] = {}
        max_update_id = last_update_id or 0

        while True:
            if ctx.is_cancelled():
                await ctx.fail("Cancelled by user")
                return

            updates = await client.get_updates(offset=offset, limit=100)
            if not updates:
                break

            for update in updates:
                update_id = update.get("update_id", 0)
                max_update_id = max(max_update_id, update_id)

                message = (
                    update.get("message")
                    or update.get("channel_post")
                    or update.get("edited_message")
                    or update.get("edited_channel_post")
                )

                if not message:
                    continue

                await ctx.increment_scanned()

                chat = message.get("chat", {})
                chat_id = chat.get("id")

                # Index chat info on first encounter
                if chat_id and chat_id not in seen_chats:
                    try:
                        chat_info = await client.get_chat(chat_id)
                        seen_chats[chat_id] = chat_info
                        docs_emitted = await self._emit_chat_doc(
                            client, chat_info, ctx, docs_emitted
                        )
                    except TelegramError as e:
                        logger.warning("Failed to get chat %s: %s", chat_id, e)
                        seen_chats[chat_id] = chat

                chat_info = seen_chats.get(chat_id, chat)

                docs_emitted = await self._emit_message_doc(
                    message, chat_info, bot_username, ctx, docs_emitted
                )

                if docs_emitted >= CHECKPOINT_INTERVAL:
                    await ctx.save_state(
                        {
                            "last_update_id": max_update_id,
                            "last_sync_at": datetime.now(timezone.utc).isoformat(),
                        }
                    )
                    docs_emitted = 0

            # Move offset past processed updates
            offset = max_update_id + 1

        await ctx.complete(
            {
                "last_update_id": max_update_id,
                "last_sync_at": datetime.now(timezone.utc).isoformat(),
            }
        )

    async def _sync_specific_chats(
        self,
        client: TelegramClient,
        bot_username: str,
        chat_ids: list[int | str],
        state: dict[str, Any],
        ctx: SyncContext,
    ) -> None:
        """Sync messages from specific chats.

        Fetches chat metadata and then processes messages via getUpdates,
        filtering to only the specified chat IDs.
        """
        docs_emitted = 0
        chat_info_map: dict[int, dict[str, Any]] = {}
        target_chat_ids: set[int] = set()

        # Index chat metadata first
        for chat_id in chat_ids:
            if ctx.is_cancelled():
                await ctx.fail("Cancelled by user")
                return

            try:
                chat_info = await client.get_chat(chat_id)
                resolved_id = chat_info.get("id", chat_id)
                chat_info_map[resolved_id] = chat_info
                target_chat_ids.add(resolved_id)

                docs_emitted = await self._emit_chat_doc(
                    client, chat_info, ctx, docs_emitted
                )
                logger.info(
                    "Indexed chat: %s (%s)",
                    chat_info.get("title") or chat_info.get("first_name"),
                    chat_id,
                )
            except TelegramError as e:
                logger.error("Failed to access chat %s: %s", chat_id, e)
                continue

        # Process messages via getUpdates, filtering to target chats
        last_update_id = state.get("last_update_id")
        offset = (last_update_id + 1) if last_update_id else None
        max_update_id = last_update_id or 0

        while True:
            if ctx.is_cancelled():
                await ctx.fail("Cancelled by user")
                return

            updates = await client.get_updates(offset=offset, limit=100)
            if not updates:
                break

            for update in updates:
                update_id = update.get("update_id", 0)
                max_update_id = max(max_update_id, update_id)

                message = (
                    update.get("message")
                    or update.get("channel_post")
                    or update.get("edited_message")
                    or update.get("edited_channel_post")
                )
                if not message:
                    continue

                await ctx.increment_scanned()

                msg_chat_id = message.get("chat", {}).get("id")
                if msg_chat_id not in target_chat_ids:
                    continue

                chat_info = chat_info_map.get(msg_chat_id, message.get("chat", {}))
                docs_emitted = await self._emit_message_doc(
                    message, chat_info, bot_username, ctx, docs_emitted
                )

                if docs_emitted >= CHECKPOINT_INTERVAL:
                    await ctx.save_state(
                        {
                            "last_update_id": max_update_id,
                            "last_sync_at": datetime.now(timezone.utc).isoformat(),
                        }
                    )
                    docs_emitted = 0

            offset = max_update_id + 1

        await ctx.complete(
            {
                "last_update_id": max_update_id,
                "last_sync_at": datetime.now(timezone.utc).isoformat(),
            }
        )

    async def _emit_chat_doc(
        self,
        client: TelegramClient,
        chat_info: dict[str, Any],
        ctx: SyncContext,
        docs_emitted: int,
    ) -> int:
        """Emit a document for a chat/group/channel."""
        try:
            member_count = await client.get_chat_member_count(chat_info["id"])
        except TelegramError:
            member_count = None

        content = generate_chat_content(chat_info, member_count)
        content_id = await ctx.content_storage.save(
            content[:MAX_CONTENT_LENGTH], "text/plain"
        )

        doc = map_chat_to_document(chat_info, content_id, member_count)
        await ctx.emit(doc)
        return docs_emitted + 1

    async def _emit_message_doc(
        self,
        message: dict[str, Any],
        chat_info: dict[str, Any],
        bot_username: str,
        ctx: SyncContext,
        docs_emitted: int,
    ) -> int:
        """Emit a document for a message."""
        content = generate_message_content(message)
        if not content or content == "[Empty message]":
            return docs_emitted

        content_id = await ctx.content_storage.save(
            content[:MAX_CONTENT_LENGTH], "text/plain"
        )

        doc = map_message_to_document(message, content_id, chat_info, bot_username)
        await ctx.emit(doc)
        return docs_emitted + 1
