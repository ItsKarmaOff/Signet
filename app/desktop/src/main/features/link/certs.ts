import { X509Certificate, createHash } from 'node:crypto'
import { generate } from 'selfsigned'
import { serverNameForLink } from '@signet/shared/link/protocol'

export interface CertPair {
  certPem: string
  keyPem: string
}

const DAY = 86_400_000

/** Self-signed ECDSA P-256 certificate for a name (the phone only pins the fingerprint: name and dates matter little). */
async function make(commonName: string): Promise<CertPair> {
  const now = Date.now()
  const pems = await generate([{ name: 'commonName', value: commonName }], {
    keyType: 'ec',
    curve: 'P-256',
    algorithm: 'sha256',
    notBeforeDate: new Date(now - DAY),
    notAfterDate: new Date(now + 3650 * DAY),
    extensions: [
      { name: 'basicConstraints', cA: false },
      { name: 'keyUsage', digitalSignature: true },
      { name: 'extKeyUsage', serverAuth: true },
      { name: 'subjectAltName', altNames: [{ type: 2, value: commonName }] }
    ]
  })
  return { certPem: pems.cert, keyPem: pems.private }
}

/** Certificate of a link (created at pairing). */
export const makeLinkCert = (linkId: string): Promise<CertPair> => make(serverNameForLink(linkId))

/** Default certificate of the installation: answers when a phone requests an unknown link. */
export const makeDefaultCert = (): Promise<CertPair> => make('default.link.signet.invalid')

/** SHA-256 of the DER certificate, in base64url (43 characters): what the phone pins. */
export function certSha256(certPem: string): string {
  return createHash('sha256').update(new X509Certificate(certPem).raw).digest('base64url')
}
