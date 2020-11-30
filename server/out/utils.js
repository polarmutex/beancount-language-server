"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.runExternalCommand = void 0;
const child_process_1 = require("child_process");
function runExternalCommand(cmd, args, callBack, opts, logger) {
    const options = opts ? opts : {};
    const child = child_process_1.spawn(cmd, args, options);
    if (logger) {
        child.on('error', (e) => logger('error: ' + e));
        child.stderr.on('date', e => logger('stderr: ' + e));
    }
    let response = '';
    child.stdout.on('data', buffer => {
        response += buffer.toString();
    });
    child.stdout.on('end', () => {
        callBack(response);
    });
}
exports.runExternalCommand = runExternalCommand;
//# sourceMappingURL=utils.js.map