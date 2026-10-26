// Expo plugin (Android prebuild). The iOS strings from `locales` (app.json) are also copied into the Android resources
// (values-b+en, values-b+fr) with no default value: Android's release check takes them for orphaned
// translations (ExtraTranslation) and blocks the build. These strings are only used on iOS: we disable this one rule.
const { withAppBuildGradle } = require('expo/config-plugins')

module.exports = function withAndroidLint(config) {
  return withAppBuildGradle(config, (c) => {
    if (!c.modResults.contents.includes("disable 'ExtraTranslation'")) {
      c.modResults.contents = c.modResults.contents.replace(/^android\s*\{/m, (m) => `${m}\n    lint {\n        disable 'ExtraTranslation'\n    }\n`)
    }
    return c
  })
}
