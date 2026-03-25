"""Postgres LISTEN/NOTIFY listener with auto-reconnect."""

import asyncio
import json
import logging
from collections.abc import Awaitable, Callable

import asyncpg

from .connection import construct_database_url

logger = logging.getLogger(__name__)

RECONNECT_DELAY_SECONDS = 2


async def start_db_listener(
    channels: dict[str, Callable[[dict], None]],
    on_reconnect: Callable[[], Awaitable[None]] | None = None,
) -> asyncio.Task:
    """Start a background task that listens for Postgres notifications.

    Args:
        channels: Mapping of channel name to callback. Callback receives the
                  parsed JSON payload dict.
        on_reconnect: Optional async callable invoked after reconnecting to
                      catch up on any notifications missed during downtime.

    Returns:
        The background asyncio.Task (cancel it to stop the listener).
    """

    async def _listener_loop():
        while True:
            conn: asyncpg.Connection | None = None
            try:
                conn = await asyncpg.connect(construct_database_url())

                def _make_handler(callback: Callable[[dict], None]):
                    def handler(
                        conn: asyncpg.Connection,
                        pid: int,
                        channel: str,
                        payload: str,
                    ):
                        try:
                            data = json.loads(payload)
                            callback(data)
                        except Exception:
                            logger.exception(
                                f"Error handling notification on '{channel}'"
                            )

                    return handler

                for channel, callback in channels.items():
                    await conn.add_listener(channel, _make_handler(callback))

                logger.info(
                    f"DB listener connected, subscribed to: {list(channels.keys())}"
                )

                # Block until the connection is closed (by Postgres or network)
                await _wait_for_close(conn)

            except asyncio.CancelledError:
                logger.info("DB listener task cancelled")
                if conn and not conn.is_closed():
                    await conn.close()
                return

            except Exception:
                logger.warning(
                    "DB listener connection lost, "
                    f"reconnecting in {RECONNECT_DELAY_SECONDS}s...",
                    exc_info=True,
                )

            finally:
                if conn and not conn.is_closed():
                    await conn.close()

            await asyncio.sleep(RECONNECT_DELAY_SECONDS)

            # Catch up on anything missed while disconnected
            if on_reconnect:
                try:
                    await on_reconnect()
                except Exception:
                    logger.exception("Error in on_reconnect callback")

    return asyncio.create_task(_listener_loop())


async def _wait_for_close(conn: asyncpg.Connection):
    """Wait until the connection is terminated."""
    while not conn.is_closed():
        await asyncio.sleep(1)
