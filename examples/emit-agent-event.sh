#!/bin/sh
# Generic Skill / custom-Agent integration example.
#
# A custom Agent that already has a lifecycle callback can emit Dock events
# without importing Dock internals.  Set a stable task id per session:
#   export AGENT_ACTIVITY_DOCK_TASK_ID="my-agent-$$"
#   dock start  "$AGENT_ACTIVITY_DOCK_TASK_ID" --source my-agent
#   ... run the Agent ...
#   dock stop   "$AGENT_ACTIVITY_DOCK_TASK_ID" --source my-agent
#   dock waiting "$AGENT_ACTIVITY_DOCK_TASK_ID" --source my-agent  # when known
#   dock error  "$AGENT_ACTIVITY_DOCK_TASK_ID" --source my-agent  # on failure
#
# The same JSON over the current-user Unix socket is:
#   {"task_id":"...","source":"my-agent","event_id":"...","action":"start"}
