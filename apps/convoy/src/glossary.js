// The words Convoy uses, each with one line that explains it. A "?" next to
// a term shows its line (ui.js `hint`), and Settings → Get started lists
// them all.
//
// Pure: no imports. Titles and lines are English keys; views call t().

export const GLOSSARY = {
  task: ["Task", "A piece of work an agent carries from start to finish, through the steps the project's How tasks run sets."],
  session: ["Session", "One agent at work in one folder: a chat or a terminal you can open, answer and resume. Tasks run in sessions too."],
  needsYou: ["Needs you", "An agent is waiting on you: a question, a permission, a plan to approve or finished work to check."],
  preset: ["How tasks run", "Which steps a project's tasks go through: plan, build, review, test and publish. Pick a preset or set each step."],
  planFirst: ["Plan first", "An agent writes a plan before any code. You approve it or ask for changes, then the build starts."],
  review: ["Review", "Another agent reads the builder's change and lists what to fix, before you look at it."],
  publish: ["Publish", "What happens to finished work: nothing, a pull request, or a push of the branch."],
  waiting: ["Waiting tasks", "Tasks saved for later. Start them and Convoy runs them in order, as many at once as the project allows."],
  worktree: ["Worktree", "A separate copy of the project on its own git branch, so an agent's changes stay apart from yours until you merge them."],
  helpers: ["Helper sessions", "A session that may start other agents in this project, give them parts of the work, wait for them and read their answers."],
  yolo: ["Skip permission prompts (Yolo)", "The agent edits files and runs commands without asking first. Fast, but nothing stops a bad command."],
  sleep: ["Sleep", "Stops an idle agent to free memory. Opening the session again resumes the same conversation."],
  skills: ["Skills", "Instructions you write once and attach to sessions, tasks and automations."],
  automation: ["Automation", "A prompt that runs on a schedule. Each run is a new session you can open later."],
  specdesk: [".specdesk folder", "Where Convoy keeps the project doc and specs inside your repository, so agents read them as files and git keeps their history."],
};

/// The order Settings → Get started lists them in.
export const GLOSSARY_ORDER = ["task", "session", "needsYou", "preset", "planFirst", "review", "publish", "waiting", "worktree", "helpers", "yolo", "sleep", "skills", "automation", "specdesk"];

/// Every title and line, for the translation check.
export const GLOSSARY_LABELS = [...Object.values(GLOSSARY).flat(), "Words Convoy uses", "What is {term}?"];
