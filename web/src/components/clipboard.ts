/** Copies text to the clipboard. The Clipboard API is unavailable over plain HTTP in some browsers;
 * there, fall back to selecting the text in a hidden textarea and the legacy copy command. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement('textarea');
    area.value = text;
    document.body.appendChild(area);
    area.select();
    document.execCommand('copy');
    area.remove();
  }
}
