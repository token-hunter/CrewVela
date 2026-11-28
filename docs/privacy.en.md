# CrewVela Privacy Policy

Last updated November 16, 2026. This notice describes how the current CrewVela application handles data. Connected agent CLIs, model providers, memory services, and tools have their own data practices.

## Local application records

CrewVela stores settings, session indexes, workspace state, collaboration messages, tasks, and run records in the application data directory and local interface storage. Depending on the session type, records may include prompts, replies, tool output, paths, and runtime errors. Context and usage displays may also save structured state locally.

## Agents, models, and tools

CrewVela uses agent CLIs installed or configured on your device. Task content, project context, and tool requests are sent to the relevant agent or service based on the action and the members you select. Whether model requests go to a remote service, and how that service retains or uses content, depends on the configuration and policies of the CLI, model provider, and tools. CrewVela does not offer a global control for these services' training or data retention practices.

Browser control, Skills, and other connected tools may access websites, project files, or external services. Review the permissions and trust settings of your tools and agents. Shared memories are stored by your configured memory service. Manage application records, native CLI history, and memory service data separately.

## Sign-in and credentials

Each local agent CLI manages its own sign-in and configuration. CrewVela uses the system credential store for some credentials it manages. This does not mean that CrewVela encrypts all third-party sign-in files, logs, or session content. Avoid including passwords, tokens, or unnecessary private information in tasks, screenshots, public issues, or feedback.

## Automatic and manual update checks

CrewVela requests public release information from GitHub after launch, periodically while it runs, and when you select Check for updates. The request includes routine HTTP information such as your source IP address and a User-Agent containing the application version. Checks and downloads do not include session content, CLI credentials, or shared memories. In builds that support in-app updates, the installer is downloaded and verified after you select Update. Release page links open GitHub in your browser.

## Managing and deleting data

Use Sessions, Archived sessions, Collaboration, Shared memory, and Browser control to manage the corresponding data and access permissions. Deletion works differently across records and services. Removing a record from CrewVela does not also clear all native CLI history, project artifacts, or data held by external services. There is currently no single export or one-click deletion action covering all of this data.

## Open source, feedback, and changes

Making the source code public does not expose the application data on your device. Logs, screenshots, project files, and release attachments you upload may include private information, so review them before sharing. For security issues, follow the repository's SECURITY.md and keep credentials out of public feedback. We will update this notice as features and service integrations change.
