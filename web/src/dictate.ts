/**
 * Dictation, through the recogniser the browser already has.
 *
 * `docs/architecture/08-web-client.md` §7. omt ships no speech key of its own,
 * and this path needs none: the browser's recogniser runs on the device or on
 * the vendor's service the user has already agreed to, and nothing omt operates
 * is in the middle of it.
 *
 * The instance's `voice.*` capabilities are the other path — a provider the
 * user supplied a key for, used where the browser has no recogniser or where
 * somebody wants a better one. Both end in the same place: text in the field,
 * for a person to read before it is sent.
 */

/** What a browser exposes, under one of two names. */
interface Recogniser {
  continuous: boolean
  interimResults: boolean
  lang: string
  start(): void
  stop(): void
  onresult: ((event: RecognitionEvent) => void) | null
  onerror: (() => void) | null
  onend: (() => void) | null
}

interface RecognitionEvent {
  resultIndex: number
  results: {
    length: number
    [index: number]: { isFinal: boolean; 0: { transcript: string } }
  }
}

type RecogniserConstructor = new () => Recogniser

/** What dictation needs from the environment. */
export interface SpeechScope {
  SpeechRecognition?: unknown
  webkitSpeechRecognition?: unknown
  isSecureContext?: boolean
}

function constructor(scope: SpeechScope): RecogniserConstructor | null {
  // Prefixed on Safari and Chrome, unprefixed in the specification. Both, so
  // this is not a feature that works only on the browser it was written in.
  const found = scope.SpeechRecognition ?? scope.webkitSpeechRecognition
  return typeof found === 'function' ? (found as RecogniserConstructor) : null
}

/** Whether this browser can dictate at all. */
export function dictationAvailable(
  scope: SpeechScope = typeof window === 'undefined' ? {} : window,
): boolean {
  // A secure context is required for the microphone, and `http://` on a LAN —
  // which is how omt is usually reached from a phone — is not one. So this is
  // false more often than the feature detection alone suggests, and the button
  // says so rather than failing on tap.
  return constructor(scope) !== null && scope.isSecureContext === true
}

/**
 * Listen until speech stops, reporting the text as it firms up.
 *
 * `onText` receives the whole transcript so far, not a fragment: a recogniser
 * revises what it heard, and appending each revision produces "open the open
 * the file open the file now".
 */
export async function dictate(
  onText: (text: string, final: boolean) => void,
  onChunk?: (text: string, final: boolean) => void,
): Promise<void> {
  const Recognition = constructor(window)
  if (Recognition === null) {
    return
  }
  const recogniser = new Recognition()
  recogniser.continuous = false
  // Interim results, because a dictation box that shows nothing until somebody
  // stops talking looks broken for the whole sentence.
  recogniser.interimResults = true
  recogniser.lang = navigator.language

  await new Promise<void>((resolve) => {
    let heard = ''
    recogniser.onresult = (event) => {
      let final = false
      let text = ''
      for (let i = 0; i < event.results.length; i += 1) {
        const result = event.results[i]
        if (result === undefined) {
          continue
        }
        text += result[0].transcript
        final = final || result.isFinal
      }
      heard = text
      onText(text, final)
      onChunk?.(text, final)
    }
    recogniser.onerror = () => resolve()
    recogniser.onend = () => {
      if (heard !== '') {
        onText(heard, true)
        onChunk?.(heard, true)
      }
      resolve()
    }
    recogniser.start()
  })
}
