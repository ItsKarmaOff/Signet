Pod::Spec.new do |s|
  s.name           = 'SignetWeb'
  s.version        = '0.1.0'
  s.summary        = 'Native content blocking (WKContentRuleList) and site data clearing for Signet.'
  s.description    = 'Local Expo module: compiles the blocking rules, attaches them to a react-native-webview WebView, clears site data.'
  s.license        = 'GPL-3.0'
  s.author         = 'Signet'
  s.homepage       = 'https://github.com/'
  s.platform       = :ios, '15.1'
  s.swift_version  = '5.4'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_COMPILATION_MODE' => 'wholemodule'
  }

  s.source_files = '*.{h,m,swift}'
  # The converted lists (see scripts/content-rules.mjs) are copied to the app root.
  s.resources = ['rules/*.json']
end
