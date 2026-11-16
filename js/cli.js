#!/usr/bin/env node
// delurio: the open checker and page of delurio-graph/0 (SPEC.md). The command line is the Rust one, run in this
// process by the addon; it writes to this process's stdout and stderr and gives the exit code.
import native from './native.cjs';

process.exitCode = native.runCli(process.argv.slice(2));
