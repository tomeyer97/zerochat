<!-- Generic preamble for pb repo-history tasks (pb.toml [harness] repo_task_prompt). pb fills the double-brace placeholders from the task's task.toml and spec file, and drops this comment. The workspace is the current directory (/workspace). -->

You are working in an existing software repository, checked out at /workspace. It is a git repository; its current commit is your starting point.

Your task is to implement the change described in the specification below, in this codebase.

<RULES>
- Implement the behaviour the specification describes. Follow the repository's existing structure, conventions and style.
- Keep existing behaviour working. Code paths the specification does not mention must behave as before.
- Do not rename, remove or change the signature of existing functions, types, fields or constants unless the specification asks for it. Hidden tests may use them.
- There is no internet access. The toolchain, dependencies and build caches are already installed; builds and tests work offline. Do not try to download anything.
- Write tests for the new behaviour and run them, together with the existing tests listed below, before you finish.
- When you are done, leave your changes in /workspace (committing them is optional).
</RULES>

## How your work is scored

After you finish, hidden tests are run against your code. They check the behaviour in the specification through the code's existing interfaces, and also run existing tests to catch regressions.

Before the hidden…[fixture: 261 more characters cut]