Analyze the content of the workspace.

If there is already a top-level `objective.md` file, then call agent.notify for role `design-lead` and stop.

Otherwise, discuss with the user what the objective should be, once you have enough information, write the objective to the file, use git to commit the change, and then call agent.notify for role `design-lead` and stop.
