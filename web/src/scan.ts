/**
 * Reading a QR with the camera.
 *
 * Built on `BarcodeDetector`, which the browser implements natively — no WASM
 * decoder shipped in the bundle. Where it does not exist the caller falls back
 * to pasting the same link, so this returns null rather than throwing: "this
 * browser cannot scan" is an ordinary answer, not a failure.
 *
 * The camera is opened only when somebody taps Add, and closed on the way out
 * of every path through here including the ones that throw. A page that holds
 * a camera open shows the recording indicator on a phone for as long as it is
 * in the foreground, and people notice.
 */

interface DetectedBarcode {
  rawValue: string
}

interface BarcodeDetectorLike {
  detect(source: CanvasImageSource): Promise<DetectedBarcode[]>
}

interface BarcodeDetectorConstructor {
  new (options?: { formats?: string[] }): BarcodeDetectorLike
}

/** Whether this browser can scan at all. */
export function canScan(
  scope: { BarcodeDetector?: unknown; isSecureContext?: boolean } = window,
): boolean {
  // A secure context is required for the camera, and `http://` on a phone is
  // not one — which is exactly how omt is usually reached on a LAN. So this is
  // false far more often than the feature detection alone would suggest, and
  // the paste path is not a rare fallback.
  return typeof scope.BarcodeDetector === 'function' && scope.isSecureContext === true
}

/** How long to keep looking before giving up and letting somebody paste. */
export const SCAN_TIMEOUT_MS = 30_000

/**
 * Show the camera until it sees a code.
 *
 * Resolves with the code's text, or null if this browser cannot scan, the
 * person cancelled, or nothing was found before the timeout.
 */
export async function scanCode(): Promise<string | null> {
  if (!canScan()) {
    return null
  }
  const Detector = (window as unknown as { BarcodeDetector: BarcodeDetectorConstructor })
    .BarcodeDetector
  const detector = new Detector({ formats: ['qr_code'] })

  let stream: MediaStream | null = null
  const overlay = document.createElement('div')
  overlay.className = 'scanner'

  try {
    stream = await navigator.mediaDevices.getUserMedia({
      // The back camera. Without this a phone opens the selfie camera and
      // somebody has to work out why pointing it at the laptop does nothing.
      video: { facingMode: 'environment' },
    })

    const video = document.createElement('video')
    video.setAttribute('playsinline', 'true')
    video.setAttribute('muted', 'true')
    video.srcObject = stream
    const cancel = document.createElement('button')
    cancel.className = 'scanner-cancel'
    cancel.textContent = 'Cancel'
    overlay.append(video, cancel)
    document.body.append(overlay)
    await video.play()

    return await new Promise<string | null>((resolve) => {
      let done = false
      const finish = (value: string | null) => {
        if (!done) {
          done = true
          resolve(value)
        }
      }
      cancel.addEventListener('click', () => finish(null))
      const deadline = setTimeout(() => finish(null), SCAN_TIMEOUT_MS)

      const look = async () => {
        if (done) {
          clearTimeout(deadline)
          return
        }
        try {
          const found = await detector.detect(video)
          const first = found[0]
          if (first) {
            clearTimeout(deadline)
            finish(first.rawValue)
            return
          }
        } catch {
          // A frame that cannot be decoded is the normal case — it is most of
          // them — and is not worth ending the scan for.
        }
        requestAnimationFrame(() => void look())
      }
      void look()
    })
  } catch {
    // Permission refused, or no camera. Either way there is a paste box.
    return null
  } finally {
    // Every path, including the throwing ones: a camera left open keeps the
    // recording indicator lit for as long as the page is in front.
    stream?.getTracks().forEach((track) => track.stop())
    overlay.remove()
  }
}
