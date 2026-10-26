Pod::Spec.new do |s|
  s.name           = 'SignetLink'
  s.version        = '0.1.0'
  s.summary        = 'Link with the computer for Signet: pinned TLS sockets, mDNS, localhost relay for the WebView.'
  s.description    = 'Local Expo module: pinned-certificate TLS sockets, Bonjour discovery, mirrored localhost ports relayed through a tunnel to the computer.'
  s.license        = 'GPL-3.0'
  s.author         = 'Signet'
  s.homepage       = 'https://github.com/'
  s.platform       = :ios, '16.4'
  s.swift_version  = '5.9'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_COMPILATION_MODE' => 'wholemodule'
  }

  s.source_files = '*.{h,m,swift}'
end
