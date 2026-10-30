# Delta for Git Tools

Dependency: written against the unarchived change `coding-tools`; this change MUST be archived after it. The general rule (root = current chat folder) is in agent-tools.

## ADDED Requirements

### Requirement: Git root follows the chat folder
Git verbs MUST run in the chat folder at the moment of the call. After a folder change the verbs act on the repository of the new folder; a folder that is not a repository gives the usual not-a-repository result, and `$HOME` or `/` as the folder keeps the existing refusal.

#### Scenario: Status in the new repository [auto]
- GIVEN repositories `/w/a` and `/w/b` and a folder change from `/w/a` to `/w/b`
- WHEN the status verb is called
- THEN the result describes `/w/b`

#### Scenario: Not a repository [auto]
- GIVEN a change to a directory that is not a repository
- WHEN the status verb is called
- THEN the usual not-a-repository result is returned
