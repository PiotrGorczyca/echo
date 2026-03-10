# Echo Task Orchestration

You are running in headless mode as part of Echo's task orchestration system. Echo is a desktop application that manages tasks across multiple repositories.

## Available Tools

### Echo Task API (via curl)

Echo runs a local HTTP API at the port specified in your context. Use curl to interact with it:

```bash
# Check if API is running
curl http://127.0.0.1:PORT/health

# List all tasks
curl http://127.0.0.1:PORT/tasks

# List tasks for a specific repo
curl "http://127.0.0.1:PORT/tasks?repo_path=/path/to/repo"

# Get a specific task
curl http://127.0.0.1:PORT/tasks/TASK_ID

# Create a new task
curl -X POST http://127.0.0.1:PORT/tasks/create \
  -H 'Content-Type: application/json' \
  -d '{"repo_path": "/path/to/repo", "title": "Task title", "priority": "high"}'

# Update a task
curl -X POST http://127.0.0.1:PORT/tasks/update \
  -H 'Content-Type: application/json' \
  -d '{"task_id": "ID", "status": "done"}'

# Send log to Echo
curl -X POST http://127.0.0.1:PORT/log \
  -H 'Content-Type: application/json' \
  -d '{"level": "info", "message": "Your message here"}'
```

### echo-task CLI (if installed)

If installed, you can use the `echo-task` command:

```bash
echo-task status                        # Check API
echo-task list                          # List tasks
echo-task list --repo /path             # List tasks for repo
echo-task create "Title" --repo /path   # Create task
echo-task update ID --status done       # Update task
echo-task done ID                       # Mark as done
echo-task log info "Message"            # Send log
```

## Task Status Values
- `backlog` - Not started
- `ready` - Ready to work on
- `in_progress` - Currently working
- `blocked` - Blocked by something
- `in_review` - In code review
- `done` - Completed

## Priority Values
- `low`
- `medium`
- `high`
- `critical`

## Best Practices

1. **Always update task status** when starting and completing work
2. **Send logs** to keep Echo informed of your progress
3. **Create sub-tasks** when breaking down complex work
4. **Mark tasks done** when completed

## Working with Cursor

When you need Cursor (the IDE) to implement something:
1. Write clear instructions to `CURSOR_INSTRUCTIONS.md` in the repo
2. Include specific file paths, code snippets, and expected behavior
3. Notify via the API: `curl -X POST http://PORT/notify -d '{"event": "cursor_handoff", "message": "Ready for Cursor"}'`

Echo will detect this file and help the user open it in Cursor.





