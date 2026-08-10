"""
An agent that speaks a real protocol, without being a real agent.

Agent nativeness is the part of omt nobody had tested end to end: the transcript
and the interaction cards are rendered from an agent's own protocol, and every
test so far fed them synthetic events from inside omt. That checks the renderer
against omt's idea of what an agent says, which is exactly the thing that can be
wrong.

This speaks the wire format the adapters parse — the method names taken from the
protocol rather than guessed — so what arrives at the transcript has been
through the adapter, the event stream, and the store, as it would from a real
binary.
"""

from __future__ import annotations

import json
import sys

#: A turn: the agent says something, calls a tool, gets a result, asks a
#: question, and finishes. Enough shape for a transcript to be wrong about.
SCRIPT = [
    {"method": "thread/started", "params": {"model": "a-test-model"}},
    {
        "method": "item/completed",
        "params": {"item": {"type": "agentMessage", "id": "m1", "text": "Reading the file."}},
    },
    {
        "method": "item/started",
        "params": {
            "item": {"type": "commandExecution", "id": "c1", "command": "cat README.md"}
        },
    },
    {
        "method": "item/completed",
        "params": {
            "item": {
                "type": "commandExecution",
                "id": "c1",
                "aggregatedOutput": "a repository under test",
            }
        },
    },
    {
        "method": "item/completed",
        "params": {
            "item": {
                "type": "fileChange",
                "id": "f1",
                "changes": [{"path": "README.md", "kind": "modified"}],
            }
        },
    },
    {
        "method": "item/completed",
        "params": {"item": {"type": "agentMessage", "id": "m2", "text": "Done."}},
    },
    {"method": "turn/completed", "params": {"turn": {"id": "t1"}}},
]


def main() -> None:
    for line in SCRIPT:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", **line}) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
