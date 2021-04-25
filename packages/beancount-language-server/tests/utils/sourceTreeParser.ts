import * as Path from "path";
import Parser from "web-tree-sitter";
import { container } from "tsyringe";

export class SourceTreeParser {
    private parser?: Parser;

    public async init(): Promise<void> {
        if (this.parser) {
            return;
        }

        await Parser.init();
        const absolute = Path.join(__dirname, "../../tree-sitter-beancount.wasm");
        const pathToWasm = Path.relative(process.cwd(), absolute);

        const language = await Parser.Language.load(pathToWasm);
        container.registerSingleton("Parser", Parser);
        this.parser = container.resolve<Parser>("Parser")
        this.parser.setLanguage(language);
    }

    public getTree(text: string): Parser.Tree | undefined {
        if (this.parser) {
            return this.parser.parse(text)
        }
        return
    }

}
