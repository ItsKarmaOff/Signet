import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'
import prettier from 'eslint-config-prettier'
import globals from 'globals'

export default tseslint.config(
  // A single configuration for the whole repository (app/desktop, packages/shared, and later app/mobile).
  {
    ignores: [
      '**/out/**',
      '**/out-e2e/**',
      '**/test-results/**',
      '**/playwright-report/**',
      '**/dist/**',
      '**/.expo/**',
      'app/mobile/ios/**',
      'app/mobile/android/**',
      '**/node_modules/**',
      'app/desktop/build/**',
      'app/desktop/resources/**'
    ]
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: [
      'app/desktop/src/main/**/*.ts',
      'app/desktop/src/preload/**/*.ts',
      'app/desktop/src/shared/**/*.ts',
      'app/desktop/*.config.ts',
      'app/desktop/electron-builder.mjs',
      'app/desktop/tests/**/*.ts',
      'app/mobile/scripts/**/*.{mjs,cjs}',
      'scripts/**/*.mjs'
    ],
    languageOptions: { globals: globals.node }
  },
  {
    files: ['app/desktop/src/renderer/**/*.{ts,tsx}'],
    languageOptions: { globals: globals.browser },
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn'
    }
  },
  {
    // Mobile app (React Native / Hermes): neither DOM nor Node in the code, but the RN globals (fetch, console, __DEV__…).
    files: ['app/mobile/src/**/*.{ts,tsx}', 'app/mobile/app/**/*.{ts,tsx}', 'app/mobile/*.ts'],
    languageOptions: {
      globals: { ...globals.es2023, __DEV__: 'readonly', fetch: 'readonly', console: 'readonly', setTimeout: 'readonly', clearTimeout: 'readonly' }
    },
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn'
    }
  },
  {
    files: ['app/mobile/*.js'],
    languageOptions: { globals: globals.node, sourceType: 'commonjs' },
    rules: { '@typescript-eslint/no-require-imports': 'off' }
  },
  {
    // Scripts injected into pages (embedded as text, see app/desktop/src/main/raw.d.ts).
    files: ['app/desktop/src/main/**/pages/**/*.js'],
    languageOptions: { globals: globals.browser }
  },
  {
    files: ['app/desktop/scripts/**/*.{js,mjs,cjs}'],
    languageOptions: { globals: globals.node }
  },
  {
    // Floating promises: a frequent source of silent errors on the main side.
    files: ['app/*/src/**/*.{ts,tsx}', 'app/mobile/app/**/*.{ts,tsx}', 'packages/*/src/**/*.{ts,tsx}'],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: {
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': ['error', { checksVoidReturn: { attributes: false } }]
    }
  },
  {
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrors: 'none' }]
    }
  },
  prettier
)
