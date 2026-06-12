# Requirements Document

## Introduction

Velotype is a lightweight code editor focused on fast, distraction-free writing and coding. Currently, code block execution happens locally on the user's machine, which requires installing language runtimes and dependencies locally. This increases the app's resource footprint and can slow down response time.

This feature enables executing code blocks on remote servers instead of locally, while keeping the app lightweight in terms of size, memory usage, and response time. Users should be able to specify server configuration per markdown document, avoid repeated authentication, and choose between external terminal integration or a lightweight built-in solution.

## Glossary

- **Velotype**: The code editor application
- **Code Block**: A section of markdown document containing executable code (fenced code blocks with language identifiers)
- **Remote Server**: A remote execution environment that runs code snippets instead of the local machine
- **Execution Context**: The environment where code is executed (local vs remote)
- **Execution Session**: A sequence of code executions sharing the same server connection and state
- **Frontmatter**: Metadata section at the beginning of markdown documents
- **Execution Manager**: Component that handles remote code execution orchestration
- **Authentication Manager**: Component that securely stores and retrieves authentication credentials
- **Connection Pool**: Component that manages reusable connections to remote servers
- **Context Manager**: Component that maintains execution state across multiple executions
- **Output Handler**: Component that routes execution results to display (built-in or external terminal)
- **Server Config**: Configuration object containing hostname, port, protocol, auth method, and timeout settings

## Requirements

### Requirement 1: Remote Code Execution Capability

**User Story:** As a user, I want to execute code blocks on remote servers, so that I don't need to install local language runtimes and dependencies.

#### Acceptance Criteria

1. WHEN a code block is executed AND remote execution is enabled, THE Velotype SHALL send the code to a configured remote server for execution
2. WHILE a remote execution is in progress, THE Velotype SHALL display execution status and progress
3. WHEN a remote execution completes, THE Velotype SHALL display the output, exit code, and execution time in the editor
4. IF the remote server is unavailable or unreachable, THEN THE Velotype SHALL display a clear error message
5. WHERE no server is configured, THE Velotype SHALL default to local execution

### Requirement 2: Lightweight App Requirements

**User Story:** As a user, I want remote code execution to minimize impact on app performance, so that Velotype remains lightweight and responsive.

#### Acceptance Criteria

1. THE total app size increase from remote execution capabilities SHALL be less than 5MB
2. WHILE executing remote code, THE Velotype SHALL use less than 100MB additional memory
3. WHEN sending code for execution, THE Velotype SHALL complete the request within 500ms for payloads under 10KB
4. WHERE a code block exceeds 100KB, THE Velotype SHALL warn the user before sending to remote server

### Requirement 3: Per-Document Server Configuration

**User Story:** As a user, I want to specify server configuration for each markdown document, so that different documents can use different execution environments.

#### Acceptance Criteria

1. WHEN a markdown document includes server configuration in frontmatter, THE Velotype SHALL use that configuration for code block execution
2. WHERE a document does not include server configuration, THE Velotype SHALL use the default global server configuration
3. IF server configuration in frontmatter is invalid, THEN THE Velotype SHALL use the global default configuration and notify the user
4. THE Server Config Format SHALL support: hostname, port, protocol (http/https), authentication method, and execution timeout

### Requirement 4: Authentication Management

**User Story:** As a user, I want to authenticate once and have my credentials reused, so that I don't need to re-enter credentials for each code execution.

#### Acceptance Criteria

1. WHEN a user provides authentication credentials for a remote server, THE Velotype SHALL store them securely
2. WHILE executing code against a configured server, THE Velotype SHALL automatically include stored authentication
3. IF stored credentials expire or become invalid, THEN THE Velotype SHALL prompt the user to re-authenticate
4. WHERE authentication fails, THE Velotype SHALL retry up to 3 times before showing an error

### Requirement 5: Execution Output Display Options

**User Story:** As a user, I want flexible options for viewing code execution results, so that I can choose the most appropriate interface for my workflow.

#### Acceptance Criteria

1. WHERE the user prefers external terminal, THE Velotype SHALL provide an option to send execution results to an external terminal
2. WHERE the user prefers built-in display, THE Velotype SHALL display execution results in a split-pane or overlay within the editor
3. WHEN execution output is displayed, THE Velotype SHALL show: code snippet, input parameters, output results, execution time, and any error messages
4. IF output exceeds 10,000 characters, THE Velotype SHALL truncate and provide a "view full output" option

### Requirement 6: Session Management

**User Story:** As a user, I want execution sessions to maintain state, so that I can run multiple related code snippets that depend on each other.

#### Acceptance Criteria

1. WHEN multiple code blocks are executed in sequence, THE Velotype SHALL maintain execution context (variables, loaded modules) between executions
2. WHERE a code block explicitly resets the context, THE Velotype SHALL clear all stored state
3. IF an execution fails, THE Velotype SHALL preserve context for subsequent executions unless explicitly reset
4. THE context persistence duration SHALL be configurable: session-only, document-open, or persistent until restart

### Requirement 7: Configuration Persistence

**User Story:** As a user, I want my server configurations to persist between sessions, so that I don't need to reconfigure them each time I use the application.

#### Acceptance Criteria

1. WHEN a server configuration is saved, THE Velotype SHALL persist it to persistent storage
2. WHEN the application restarts, THE Velotype SHALL load previously saved server configurations
3. WHERE a configuration has been deleted, THE Velotype SHALL remove it from persistent storage
4. THE configuration storage SHALL support concurrent access from multiple documents

### Requirement 8: Connection Pool Management

**User Story:** As a user, I want connections to be reused efficiently, so that multiple code executions don't each create new connections.

#### Acceptance Criteria

1. WHEN multiple code executions target the same server, THE Velotype SHALL reuse existing connections from the pool
2. WHERE a connection becomes stale or invalid, THE Velotype SHALL establish a new connection
3. WHEN no executions are in progress, THE Velotype SHALL maintain idle connections for a configurable duration
4. THE connection pool SHALL limit the maximum number of concurrent connections per server

### Requirement 9: Context Isolation

**User Story:** As a user, I want execution contexts to be isolated between documents, so that code from one document doesn't interfere with another.

#### Acceptance Criteria

1. WHEN code blocks are executed from different documents, THE Velotype SHALL maintain separate execution contexts
2. WHERE a document specifies a custom context ID, THE Velotype SHALL use that ID for context isolation
3. IF no custom context ID is specified, THE Velotype SHALL derive the context ID from the document path
4. CONTEXT data from one document SHALL NOT be accessible to another document

### Requirement 10: Timeout Configuration

**User Story:** As a user, I want configurable execution timeouts, so that long-running code doesn't hang indefinitely.

#### Acceptance Criteria

1. WHEN code execution begins, THE Velotype SHALL apply the configured timeout from the server configuration
2. WHERE no timeout is specified, THE Velotype SHALL use a default timeout of 30 seconds
3. IF execution exceeds the configured timeout, THE Velotype SHALL cancel the execution and return a timeout error
4. THE timeout SHALL be configurable per-server and per-document

### Requirement 11: Secure Credential Storage

**User Story:** As a user, I want my authentication credentials to be stored securely, so that they are not exposed to other applications or users.

#### Acceptance Criteria

1. WHEN credentials are stored, THE Velotype SHALL encrypt them before persistent storage
2. WHERE credentials are retrieved, THE Velotype SHALL decrypt them only in memory
3. THE application SHALL NOT log or display credentials in any form
4. CREDENTIALS SHALL be isolated by storage key and not accessible across different server configurations

### Requirement 12: Error Handling and Recovery

**User Story:** As a user, I want robust error handling, so that execution failures don't crash the application.

#### Acceptance Criteria

1. WHEN a remote execution fails, THE Velotype SHALL catch the error and display a user-friendly message
2. IF the connection pool encounters an error, THE Velotype SHALL retry the operation before failing
3. WHERE an unrecoverable error occurs, THE Velotype SHALL clean up resources and return to idle state
4. ALL errors SHALL include context information (document path, code language, server configuration)