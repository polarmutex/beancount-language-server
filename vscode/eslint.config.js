const parser = require("@typescript-eslint/parser");
const eslintPlugin = require("@typescript-eslint/eslint-plugin");

const typeChecked = eslintPlugin.configs["recommended-type-checked"];

module.exports = [
  {
    ignores: ["out/**", "node_modules/**"],
  },
  {
    ...typeChecked,
    files: ["**/*.ts"],
    languageOptions: {
      ...(typeChecked.languageOptions ?? {}),
      parser,
      parserOptions: {
        ...(typeChecked.languageOptions?.parserOptions ?? {}),
        project: "./tsconfig.eslint.json",
        tsconfigRootDir: __dirname,
        sourceType: "module",
      },
    },
    plugins: {
      ...(typeChecked.plugins ?? {}),
      "@typescript-eslint": eslintPlugin,
    },
    rules: {
      ...(typeChecked.rules ?? {}),
      eqeqeq: ["error", "always", { null: "ignore" }],
      "no-console": ["error"],
      "prefer-const": "error",
      "@typescript-eslint/member-delimiter-style": [
        "error",
        {
          multiline: {
            delimiter: "semi",
            requireLast: true,
          },
          singleline: {
            delimiter: "semi",
            requireLast: false,
          },
        },
      ],
      "@typescript-eslint/semi": ["error", "always"],
      "@typescript-eslint/no-unnecessary-type-assertion": "error",
    },
  },
];
