// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// ESLint's own config file must stay plain JavaScript: ESLint loads it before
// any TypeScript tooling is available. It is the only .js file in the
// repository, and scripts/check-language.ts allows it by exact path.

import js from "@eslint/js";
import tseslint from "typescript-eslint";

const IGNORED = [
  "node_modules/**",
  "target/**",
  "dist/**",
  ".turbo/**",
  ".codegraph*/**",
];

// Rules this repository applies to its own TypeScript. The coding rules in
// AGENTS.md appear here where a lint can express them.
const projectRules = {
  "max-lines-per-function": [
    "warn",
    { max: 60, skipBlankLines: true, skipComments: true },
  ],
  "max-depth": ["warn", 3],
  "no-magic-numbers": ["warn", { ignore: [0, 1], ignoreArrayIndexes: true }],
  "no-else-return": "warn",
  "consistent-return": "warn",
  "@typescript-eslint/explicit-function-return-type": "warn",
};

export default tseslint.config(
  { ignores: IGNORED },
  js.configs.recommended,
  {
    files: ["**/*.ts"],
    extends: [...tseslint.configs.strictTypeChecked],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: projectRules,
  },
  {
    files: ["eslint.config.js"],
    languageOptions: {
      sourceType: "module",
    },
  },
);
