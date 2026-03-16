# Project workflow

Use Beads for task tracking.
Use TaskWing for planning.

Workflow:

1. Check available tasks:
   bd ready

2. Claim a task:
   bd update <id> --claim

3. If task requires planning:
   call TaskWing to create subtasks.

4. Implement only the current task.

5. If new work appears:
   create follow-up tasks with bd create.

6. Close task only when tests pass.
