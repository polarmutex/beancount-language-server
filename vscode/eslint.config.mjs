import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

import prettierConfig from "eslint-config-prettier";
import parser from "@typescript-eslint/parser";
import { defineConfig } from "eslint/config";
import tseslint from "typescript-eslint";
import simpleImportSort from "eslint-plugin-simple-import-sort";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

export default defineConfig(
  {
    ignores: ["out/**", "node_modules/**"],
  },
  prettierConfig,
  ...tseslint.configs.recommendedTypeChecked,
  {
    files: ["**/*.ts"],
    languageOptions: {
      parser,
      parserOptions: {
        project: "./tsconfig.eslint.json",
        tsconfigRootDir: __dirname,
        sourceType: "module",
      },
    },
    rules: {
      eqeqeq: ["error", "always", { null: "ignore" }],
      "no-console": ["error"],
      "prefer-const": "error",
      "@typescript-eslint/no-unnecessary-type-assertion": "error",
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
    },
  },
  {
    files: ["**/*.ts", "**/*.js", "**/*.mjs"],
    plugins: {
      "simple-import-sort": simpleImportSort,
    },
    rules: {
      "simple-import-sort/imports": "error",
      "simple-import-sort/exports": "error",
    },
  },
);
