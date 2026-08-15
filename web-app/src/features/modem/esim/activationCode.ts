// Activation code handling, per GSMA SGP.22 §4.1.
//
// The user rarely has a clean activation code. They have a QR photo, a link
// from an operator email, or three fields off a web page. All three end up as
// the same thing, so parsing is done once here rather than in each input mode.

export interface ParsedActivationCode {
  smdp: string
  matchingId: string
  /** SM-DP+ OID, present on some codes. The agent does not use it. */
  smdpOid?: string
  /** The operator will demand a confirmation code before releasing the profile. */
  confirmationCodeRequired: boolean
}

/**
 * Pull an activation code out of whatever the user pasted or scanned.
 *
 * Accepts the bare `LPA:1$...` form, the same without the `LPA:` scheme (some
 * QR codes omit it), and the universal links operators hand out — Apple's
 * `esimsetup.apple.com/...?carddata=LPA:1$...` and Android's
 * `?data=LPA:1$...`, which is what you get from scanning with a phone camera
 * and copying the link rather than the code.
 */
export function parseActivationCode(raw: string): ParsedActivationCode | null {
  let text = raw.trim()
  if (!text) return null

  if (/^https?:\/\//i.test(text)) {
    let query: URLSearchParams
    try {
      query = new URL(text).searchParams
    } catch {
      return null
    }
    const embedded = query.get('carddata') ?? query.get('data') ?? query.get('lpa')
    if (!embedded) return null
    text = embedded.trim()
  }

  if (/^lpa:/i.test(text)) text = text.slice(4).trim()

  const parts = text.split('$')
  // Format version, SM-DP+ address, matching ID, then two optional fields.
  if (parts.length < 3 || parts[0] !== '1') return null

  const smdp = parts[1].trim()
  const matchingId = parts[2].trim()
  if (!smdp || !matchingId) return null

  return {
    smdp,
    matchingId,
    smdpOid: parts[3]?.trim() || undefined,
    // "1" is the only value that means required; anything else, including an
    // absent field, means it is not.
    confirmationCodeRequired: parts[4]?.trim() === '1',
  }
}

/**
 * Decode a QR code from an image file, entirely in the browser.
 *
 * The image never leaves the machine — it is drawn to a canvas and the pixels
 * are read directly. That matters because an activation code is a bearer
 * credential: whoever holds it can claim the profile, once.
 *
 * jsQR is imported dynamically so the 130 KB decoder is only fetched when
 * someone actually scans an image. This dashboard is served off the router's
 * flash and most sessions never open this tab.
 */
export async function decodeQrImage(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file).catch(() => {
    throw new Error('That file could not be read as an image.')
  })

  // Very large photos cost decode time for no accuracy gain, and phone cameras
  // routinely produce 4000px images of a 200px QR code.
  const scale = Math.min(1, 1600 / Math.max(bitmap.width, bitmap.height))
  const width = Math.round(bitmap.width * scale)
  const height = Math.round(bitmap.height * scale)

  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const context = canvas.getContext('2d', { willReadFrequently: true })
  if (!context) throw new Error('This browser cannot decode images.')
  context.drawImage(bitmap, 0, 0, width, height)
  bitmap.close()

  const { data } = context.getImageData(0, 0, width, height)
  const { default: jsQR } = await import('jsqr')

  // Screenshots of QR codes are often inverted or on a dark background, so try
  // both polarities before giving up.
  const found =
    jsQR(data, width, height, { inversionAttempts: 'attemptBoth' }) ??
    jsQR(data, width, height, { inversionAttempts: 'invertFirst' })

  if (!found?.data) {
    throw new Error('No QR code found in that image. Try a tighter, sharper crop.')
  }
  return found.data
}
