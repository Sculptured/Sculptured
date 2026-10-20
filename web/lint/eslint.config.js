import js from '@eslint/js';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import tseslint from 'typescript-eslint';

const FUNCTION_LINES = { max: 30, skipBlankLines: true, skipComments: true };

export default tseslint.config(
  { ignores: ['dist', 'lint', 'test-results', 'playwright-report', 'src/generated'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
      'max-lines-per-function': ['error', FUNCTION_LINES],
      'max-lines': ['error', { max: 300, skipBlankLines: true, skipComments: true }],
    },
  },
  {
    files: ['**/*.test.{ts,tsx}', 'e2e/**', 'src/test-fixtures.ts', 'src/test-setup.ts'],
    rules: { 'max-lines-per-function': 'off' },
  },
);
