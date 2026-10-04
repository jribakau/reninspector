export interface AskOption {
  value: string
  label: string
}

export interface AskField {
  label: string
  value?: string
  placeholder?: string
  /** Shows a Browse button that fills the field from a picker. */
  browse?: () => Promise<string | null>
  /** When set, the field is a dropdown. Browse can still set a value outside the list. */
  options?: AskOption[]
  optional?: boolean
}

export interface AskRequest {
  title: string
  note?: string
  ok?: string
  /** A third button, between OK and Cancel. */
  alt?: string
  fields: AskField[]
}

interface Pending extends AskRequest {
  values: string[]
  resolve: (values: string[] | null | 'alt') => void
}

export const dialog = $state<{ current: Pending | null }>({ current: null })

/** Resolves with one trimmed value per field, or null on Cancel. A newer request cancels this one. */
export function ask(request: AskRequest): Promise<string[] | null> {
  dialog.current?.resolve(null)
  return new Promise((resolve) => {
    dialog.current = {
      ...request,
      values: request.fields.map((f) => f.value ?? ''),
      resolve: (values) => resolve(values === 'alt' ? null : values),
    }
  })
}

/** Save, discard, or cancel. Used when a tab with unsaved edits is closing. */
export function askSaveDiscard(title: string, note?: string): Promise<'save' | 'discard' | 'cancel'> {
  dialog.current?.resolve(null)
  return new Promise((resolve) => {
    dialog.current = {
      title,
      note,
      ok: 'Save',
      alt: 'Discard',
      fields: [],
      values: [],
      resolve: (values) => {
        if (values === 'alt') resolve('discard')
        else if (values) resolve('save')
        else resolve('cancel')
      },
    }
  })
}

export async function askText(title: string, value = '', ok = 'OK'): Promise<string | null> {
  const got = await ask({ title, ok, fields: [{ label: '', value }] })
  return got ? got[0] : null
}

export function closeDialog(submit: boolean | 'alt') {
  const pending = dialog.current
  if (!pending) return
  dialog.current = null
  if (submit === 'alt') pending.resolve('alt')
  else pending.resolve(submit ? pending.values.map((v) => v.trim()) : null)
}
