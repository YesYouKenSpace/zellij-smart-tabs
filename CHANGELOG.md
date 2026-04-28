# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- feat: configurable skip_programs to skip wrapper commands (e.g. sudo) during program detection
- feat: display terminal_command and running_command in Panes UI view
- fix: remove stray pipe separator from default format when status is empty
- feat: validate user format templates at config load, fall back to default with warning on parse error
- security: Workflow does not contain permissions (#9)
- fix: removed persistence code since it is not working yet (#8)

## [0.1.0] - 2026-04-08

- feat: detect zellij version for incompatability (#6)
- doc: delete demo video (#5)
- doc: demo reference (#3)
- doc: added demo and CHANGELOG.md (#2)
- chore: Create SECURITY.md

## [0.0.2] - 2026-04-07

- feat: auto renaming
