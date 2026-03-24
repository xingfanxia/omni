"""Integration tests for the /prompt endpoint."""

import pytest


@pytest.mark.integration
async def test_prompt_non_streaming_returns_mock_response(
    async_client, mock_llm_provider
):
    """Verify non-streaming response contains expected mock text."""
    response = await async_client.post(
        "/prompt", json={"prompt": "Say hello", "stream": False}
    )
    assert response.status_code == 200
    data = response.json()

    # Mock returns "This is a test response from the mock LLM."
    assert data["response"] == "This is a test response from the mock LLM."


@pytest.mark.integration
async def test_prompt_streaming_returns_text_chunks(async_client):
    """Verify streaming response yields text content."""
    async with async_client.stream(
        "POST", "/prompt", json={"prompt": "Say hello", "stream": True}
    ) as response:
        assert response.status_code == 200
        assert "text/plain" in response.headers["content-type"]

        chunks = []
        async for chunk in response.aiter_text():
            chunks.append(chunk)

        # Mock yields "Streamed response"
        assert "".join(chunks) == "Streamed response"


@pytest.mark.integration
async def test_prompt_passes_parameters_to_provider(async_client, mock_llm_provider):
    """Verify temperature, max_tokens, top_p are passed to provider."""
    await async_client.post(
        "/prompt",
        json={
            "prompt": "Test",
            "stream": False,
            "max_tokens": 100,
            "temperature": 0.5,
            "top_p": 0.9,
        },
    )

    # Verify mock was called with correct params
    mock_llm_provider.generate_response.assert_called_once()
    call_kwargs = mock_llm_provider.generate_response.call_args.kwargs
    assert call_kwargs["max_tokens"] == 100
    assert call_kwargs["temperature"] == 0.5
    assert call_kwargs["top_p"] == 0.9


@pytest.mark.integration
async def test_prompt_missing_prompt_field_returns_422(async_client):
    """Missing required 'prompt' field should return validation error."""
    response = await async_client.post("/prompt", json={"stream": False})
    assert response.status_code == 422
