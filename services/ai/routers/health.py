"""Health check endpoint."""

import logging

from fastapi import APIRouter, Request

from config import PORT
from db_config import get_embedding_config

logger = logging.getLogger(__name__)
router = APIRouter(tags=["health"])


@router.get("/health")
async def health_check(request: Request):
    """Health check endpoint."""
    # Check LLM provider health (check default model)
    llm_health = False
    llm_provider_name = "none"
    llm_model_name = "none"
    try:
        resolved = await request.app.state.provider_cache.resolve_default()
        if resolved is not None:
            provider = resolved.provider
            llm_provider_name = type(provider).__name__
            llm_model_name = resolved.model_name
            llm_health = await provider.health_check()
    except Exception:
        logger.warning("Default model health check failed")

    # Get embedding model name from provider
    embedding_model = (
        request.app.state.embedding_provider.get_model_name()
        if request.app.state.embedding_provider is not None
        else "unknown"
    )

    # Get current configurations
    embedding_config = await get_embedding_config()

    return {
        "status": "healthy",
        "service": "ai",
        "embedding_provider": embedding_config.provider if embedding_config else "none",
        "embedding_model": embedding_model,
        "port": PORT,
        "embedding_dimensions": (embedding_config.dimensions if embedding_config else None),
        "llm_provider": llm_provider_name,
        "llm_model": llm_model_name,
        "llm_health": llm_health,
    }
