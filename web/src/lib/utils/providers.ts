const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
    anthropic: 'Anthropic',
    openai: 'OpenAI',
    vllm: 'vLLM',
    bedrock: 'Bedrock',
    gemini: 'Gemini',
    azure_foundry: 'Azure AI',
    vertex_ai: 'Vertex AI',
}

export function formatProviderName(provider: string): string {
    return PROVIDER_DISPLAY_NAMES[provider] ?? provider.charAt(0).toUpperCase() + provider.slice(1)
}
