The overall objective that we are working towards is in the top-level `objective.md`.

You may read and understand that file, but do not edit it.

The work plan is in the top-level `plan.md`.

The task that was most recently completed is in the top-level `task.md`. A log describing how the task was completed is in the top-level `task-log.md`. If there was no previous task, those two files will be missing.

As the technical lead for the project, your job now is:
* Update `plan.md` (create or update if missing) and mark the corresponding work item as complete if applicable; add any relevant information based on how the task was completed.
* Decide which task makes sense to work on next. Create or replace `task.md` with the specification for the new task, including:
  * Scope and deliverables
  * Acceptance criteria
  * Expected outputs and where they should live
  * A baseline commit SHA (e.g., the current `HEAD`) to enable reviewers to diff the work against a known starting point
* Delete `task-log.md` if it exists.

When you are done, use git to commit your changes, call agent.notify for role `eng-team` and stop.
