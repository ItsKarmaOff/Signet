// The only non-ECMAScript global this package needs (present in Electron, Node, browsers and Hermes with a polyfill).
// The package is compiled without DOM or Node types; in consumers, those types already provide `URL`.
declare class URL {
  constructor(url: string, base?: string)
  readonly href: string
  readonly origin: string
  protocol: string
  readonly host: string
  readonly hostname: string
  readonly port: string
  readonly pathname: string
  readonly search: string
  readonly hash: string
}
