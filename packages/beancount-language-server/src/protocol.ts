import { RequestType } from "vscode-languageserver"

export const BeanCheckRequest = new RequestType<void, void, void>("beancount/beanCheck");
