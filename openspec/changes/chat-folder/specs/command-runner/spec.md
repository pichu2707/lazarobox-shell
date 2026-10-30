# Delta for Command Runner

Dependency: written against the unarchived change `coding-tools`; this change MUST be archived after it. The general rule (root = current chat folder) is in agent-tools.

## ADDED Requirements

### Requirement: Run folder follows the chat folder
The working directory shown in the approval prompt and used to start a command MUST be inside the chat folder at the moment of the call. A `run_command` whose approval prompt is open or whose run is active MUST keep the folder it started with; a folder change is refused while either holds (see chat-folder), so a prompt can never show one folder and run in another. The refusals of "Refusals before start" apply to the new folder as to any.

#### Scenario: Prompt shows the new folder [auto]
- GIVEN a folder change to `/w/b`
- WHEN `run_command` is called
- THEN the prompt shows `/w/b` as the working directory

#### Scenario: Change refused during a prompt or run [auto]
- GIVEN an approval prompt pending, and in another run a command running
- WHEN `/cd /w/c` is requested
- THEN it is refused with the matching reason and the prompt or run keeps its folder
