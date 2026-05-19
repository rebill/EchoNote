const PCM_MAX = 0x7fff;
const PCM_MIN = -0x8000;

export function encodePcm16Wav(samples: Float32Array, sampleRate: 16000 = 16000): ArrayBuffer {
  const bytesPerSample = 2;
  const channelCount = 1;
  const dataByteLength = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataByteLength);
  const view = new DataView(buffer);

  writeAscii(view, 0, "RIFF");
  view.setUint32(4, 36 + dataByteLength, true);
  writeAscii(view, 8, "WAVE");
  writeAscii(view, 12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, channelCount, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channelCount * bytesPerSample, true);
  view.setUint16(32, channelCount * bytesPerSample, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, "data");
  view.setUint32(40, dataByteLength, true);

  let offset = 44;
  for (const sample of samples) {
    const clamped = Math.max(-1, Math.min(1, sample));
    const int16 = clamped < 0 ? clamped * -PCM_MIN : clamped * PCM_MAX;
    view.setInt16(offset, int16, true);
    offset += bytesPerSample;
  }

  return buffer;
}

export function concatWavFiles(wavFiles: ArrayBuffer[], sampleRate: 16000 = 16000): ArrayBuffer {
  const samples: Int16Array[] = [];
  let totalSamples = 0;

  for (const wavFile of wavFiles) {
    if (wavFile.byteLength <= 44) {
      continue;
    }

    const pcmBytes = wavFile.slice(44);
    const pcm = new Int16Array(pcmBytes);
    samples.push(pcm);
    totalSamples += pcm.length;
  }

  const bytesPerSample = 2;
  const dataByteLength = totalSamples * bytesPerSample;
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

  let offset = 44;
  for (const pcm of samples) {
    for (const sample of pcm) {
      view.setInt16(offset, sample, true);
      offset += bytesPerSample;
    }
  }

  return buffer;
}

function writeAscii(view: DataView, offset: number, value: string): void {
  for (let i = 0; i < value.length; i += 1) {
    view.setUint8(offset + i, value.charCodeAt(i));
  }
}
