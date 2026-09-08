from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from routers.health import health_check


@pytest.mark.asyncio
async def test_health_uses_current_database_backed_default(monkeypatch):
    provider = SimpleNamespace(health_check=AsyncMock(return_value=True))
    resolve = AsyncMock(
        return_value=SimpleNamespace(
            provider=provider,
            model_name="configured-model",
        )
    )
    state = SimpleNamespace(
        provider_cache=SimpleNamespace(resolve_default=resolve),
        embedding_provider=None,
    )
    monkeypatch.setattr("routers.health.get_embedding_config", AsyncMock(return_value=None))

    result = await health_check(SimpleNamespace(app=SimpleNamespace(state=state)))

    resolve.assert_awaited_once()
    provider.health_check.assert_awaited_once()
    assert result["llm_model"] == "configured-model"
    assert result["llm_health"] is True
    assert result["embedding_model"] == "unknown"


@pytest.mark.asyncio
@pytest.mark.parametrize("failure", ["missing", "resolve", "provider"])
async def test_health_reports_unavailable_default_without_raising(monkeypatch, failure):
    provider = SimpleNamespace(health_check=AsyncMock(side_effect=RuntimeError("unavailable")))
    resolve = AsyncMock(
        return_value=None
        if failure == "missing"
        else SimpleNamespace(
            provider=provider,
            model_name="configured-model",
        )
    )
    if failure == "resolve":
        resolve.side_effect = RuntimeError("database unavailable")
    state = SimpleNamespace(
        provider_cache=SimpleNamespace(resolve_default=resolve),
        embedding_provider=None,
    )
    monkeypatch.setattr("routers.health.get_embedding_config", AsyncMock(return_value=None))

    result = await health_check(SimpleNamespace(app=SimpleNamespace(state=state)))

    assert result["llm_health"] is False
    assert result["embedding_provider"] == "none"
