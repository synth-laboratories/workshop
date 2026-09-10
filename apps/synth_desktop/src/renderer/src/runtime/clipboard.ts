export async function copyText(value: string): Promise<void> {
	if (!value) throw new Error("There is no text to copy");
	if (navigator.clipboard?.writeText) {
		try {
			await navigator.clipboard.writeText(value);
			return;
		} catch {
			// Tauri/WebKit can deny the async clipboard API even for a direct click.
			// Continue to the synchronous, gesture-bound path below.
		}
	}

	const textarea = document.createElement("textarea");
	textarea.value = value;
	textarea.setAttribute("readonly", "");
	textarea.style.position = "fixed";
	textarea.style.inset = "0 auto auto -10000px";
	document.body.append(textarea);
	textarea.select();
	const copied = document.execCommand("copy");
	textarea.remove();
	if (!copied) throw new Error("Clipboard access is unavailable");
}
