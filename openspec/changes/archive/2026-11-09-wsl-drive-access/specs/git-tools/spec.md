# Git Tools Specification (delta: WSL drive access)

## MODIFIED Requirements

### Requirement: Git root follows the chat folder
Git verbs MUST run in the chat folder at the moment of the call. After a folder change the verbs act on the repository of the new folder; a folder that is not a repository gives the usual not-a-repository result, and `$HOME` or `/` as the folder keeps the existing refusal. Under WSL, a Windows drive folder follows the grant: without it every verb returns the folder-level drive text of code-write, Root policy; with it the verbs run there like in any root.

#### Scenario: Status in the new repository [auto]
- GIVEN repositories `/w/a` and `/w/b` and a folder change from `/w/a` to `/w/b`
- WHEN the status verb is called
- THEN the result describes `/w/b`

#### Scenario: Not a repository [auto]
- GIVEN a change to a directory that is not a repository
- WHEN the status verb is called
- THEN the usual not-a-repository result is returned

#### Scenario: Git in a drive folder [auto]
- GIVEN WSL and a repository at `/mnt/c/Users/ana/Documents/app` as the chat folder
- WHEN the status verb is called before and after the user grants the folder
- THEN the first returns the folder-level drive text and the second the status of that repository
