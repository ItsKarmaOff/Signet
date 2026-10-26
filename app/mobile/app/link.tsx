// signet://link?d=…: link from the pairing QR code (the iOS camera opens the app with it). Confirmation sheet.
import { PairConfirmScreen } from '@/features/link'

export default function LinkRoute() {
  return <PairConfirmScreen sheet />
}
