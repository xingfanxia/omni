"""A minimal MCP server used as a subprocess for testing the stdio adapter."""

import sys

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

server = FastMCP("test")


@server.tool(annotations=ToolAnnotations(readOnlyHint=True))
def greet(name: str) -> str:
    """Greet someone by name."""
    return f"Hello, {name}!"


@server.tool()
def add(a: int, b: int) -> str:
    """Add two numbers."""
    return str(a + b)


@server.resource("test://item/{item_id}")
def get_item(item_id: str) -> str:
    """Get an item by ID."""
    return f"Item {item_id}"


@server.prompt()
def summarize(text: str) -> str:
    """Summarize the given text."""
    return f"Please summarize: {text}"


if __name__ == "__main__":
    server.run(transport="stdio")
