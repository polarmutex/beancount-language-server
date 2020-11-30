/// <reference types="node" />
import { SpawnOptions } from 'child_process';
export declare function runExternalCommand(cmd: string, args: string[], callBack: (stdout: string) => void, opts?: SpawnOptions, logger?: (str: string) => void): void;
