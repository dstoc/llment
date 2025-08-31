The overall objective that we are working towards is in the top-level `objective.md`.

The work plan is in the top-level `plan.md`.

Your peer's current task is in the top-level `task.md`.

Your peer's work summary is in the top-level `task-log.md`. If they have done their job, all the tasks should be complete.

You may read and understand those files, but do not edit them.

Your peer has submitted their work to you for review. To review the changes:
* If `task.md` specifies a baseline commit SHA, use it to generate the diff (e.g., `git diff <baseline_sha>..HEAD`).
* If a baseline is not specified, derive the diff from the files in scope described in `task.md` (e.g., inspect the most recent commits touching those files and review their diffs).

Review the work and decide whether it meets the requirements of the task.

If the work is satisfactory, call agent.notify for role `execution-lead`, and request that they assign the next task; if there are any deviations from the task, summarize them, and stop.

Otherwise, call agent.notify for role `eng-team`, and summarize any problems or changes that are necessary, and stop.
