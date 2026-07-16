const PCM_MAX = 0x7fff;
const PCM_MIN = -0x8000;

export function encodePcm16Wav(samples: Float32Array, sampleRate: number = 16000): ArrayBuffer {
  const bytesPerSample = 2;
  const dataByteLength = samples.length * bytesPerSample;
  const buffer = createPcm16WavBuffer(dataByteLength, sampleRate);
  const view = new DataView(buffer);

  let offset = 44;
  for (const sample of samples) {
    const clamped = Math.max(-1, Math.min(1, sample));
    const int16 = clamped < 0 ? clamped * -PCM_MIN : clamped * PCM_MAX;
    view.setInt16(offset, int16, true);
    offset += bytesPerSample;
  }

  return buffer;
}

export function concatWavFiles(wavFiles: ArrayBuffer[], sampleRate: number = 16000): ArrayBuffer {
  const payloads: Uint8Array[] = [];
  let dataByteLength = 0;

  for (const wavFile of wavFiles) {
    if (wavFile.byteLength <= 44) {
      continue;
    }

    const payload = new Uint8Array(wavFile, 44);
    payloads.push(payload);
    dataByteLength += payload.byteLength;
  }

  const buffer = createPcm16WavBuffer(dataByteLength, sampleRate);

  const output = new Uint8Array(buffer);
  let offset = 44;
  for (const payload of payloads) {
    output.set(payload, offset);
    offset += payload.byteLength;
  }

  return buffer;
}

export function createPcm16WavBuffer(dataByteLength: number, sampleRate: number = 16000): ArrayBuffer {
  if (!Number.isInteger(dataByteLength) || dataByteLength < 0 || dataByteLength % 2 !== 0) {
    throw new Error("PCM16 data byte length must be a non-negative even integer.");
  }

  const bytesPerSample = 2;
  const buffer = new ArrayBuffer(44 + dataByteLength);
  const view = new DataView(buffer);
  writeAscii(view, 0, "RIFF");
  view.setUint32(4, 36 + dataByteLength, true);
  writeAscii(view, 8, "WAVE");
  writeAscii(view, 12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * bytesPerSample, true);
  view.setUint16(32, bytesPerSample, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, "data");
  view.setUint32(40, dataByteLength, true);
  return buffer;
}

function writeAscii(view: DataView, offset: number, value: string): void {
  for (let i = 0; i < value.length; i += 1) {
    view.setUint8(offset + i, value.charCodeAt(i));
  }
}
