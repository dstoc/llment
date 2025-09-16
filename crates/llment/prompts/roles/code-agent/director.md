Analyze the content of the workspace.

If there exists a top-level `task.md` file, then call agent_notify for role `eng-team`, ask them to resume their work, and stop.

If there is already a top-level `objective.md` file, then call agent_notify for role `design-lead` and stop.

Otherwise, discuss with the user what the objective should be; once you have enough information, after confirming with the user, write the objective to the top-level `objective.md`, use git to commit the change, and then call agent_notify for role `design-lead` and stop.
