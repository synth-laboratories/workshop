export function blobToBase64(blob: Blob): Promise<string> {
	return new Promise((resolve, reject) => {
		const reader = new FileReader();
		reader.onloadend = () => {
			const result = reader.result;
			if (typeof result !== "string") {
				reject(new Error("Unexpected FileReader result"));
				return;
			}
			const commaIndex = result.indexOf(",");
			resolve(commaIndex >= 0 ? result.slice(commaIndex + 1) : result);
		};
		reader.onerror = () => reject(reader.error ?? new Error("Failed to read recorded audio"));
		reader.readAsDataURL(blob);
	});
}

export async function recordingToWhisperWav(blob: Blob): Promise<Blob> {
	const context = new AudioContext();
	try {
		const decoded = await context.decodeAudioData(await blob.arrayBuffer());
		const targetRate = 16_000;
		const outputLength = Math.max(1, Math.round(decoded.duration * targetRate));
		const pcm = new Float32Array(outputLength);
		for (let outputIndex = 0; outputIndex < outputLength; outputIndex += 1) {
			const sourceIndex = Math.min(decoded.length - 1, Math.floor((outputIndex * decoded.sampleRate) / targetRate));
			let sample = 0;
			for (let channel = 0; channel < decoded.numberOfChannels; channel += 1) {
				sample += decoded.getChannelData(channel)[sourceIndex] ?? 0;
			}
			pcm[outputIndex] = sample / decoded.numberOfChannels;
		}

		const wav = new ArrayBuffer(44 + pcm.length * 2);
		const view = new DataView(wav);
		const writeAscii = (offset: number, value: string) => {
			for (let index = 0; index < value.length; index += 1) view.setUint8(offset + index, value.charCodeAt(index));
		};
		writeAscii(0, "RIFF");
		view.setUint32(4, 36 + pcm.length * 2, true);
		writeAscii(8, "WAVE");
		writeAscii(12, "fmt ");
		view.setUint32(16, 16, true);
		view.setUint16(20, 1, true);
		view.setUint16(22, 1, true);
		view.setUint32(24, targetRate, true);
		view.setUint32(28, targetRate * 2, true);
		view.setUint16(32, 2, true);
		view.setUint16(34, 16, true);
		writeAscii(36, "data");
		view.setUint32(40, pcm.length * 2, true);
		for (let index = 0; index < pcm.length; index += 1) {
			const sample = Math.max(-1, Math.min(1, pcm[index]));
			view.setInt16(44 + index * 2, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
		}
		return new Blob([wav], { type: "audio/wav" });
	} finally {
		await context.close();
	}
}
