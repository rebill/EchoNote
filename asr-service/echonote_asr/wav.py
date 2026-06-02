from __future__ import annotations

import io
import wave

from fastapi import HTTPException, UploadFile, status


MAX_WAV_BYTES = 512 * 1024 * 1024
ALLOWED_WAV_CONTENT_TYPES = {
    None,
    "",
    "application/octet-stream",
    "audio/wav",
    "audio/wave",
    "audio/x-wav",
    "audio/vnd.wave",
}


async def read_valid_wav(file: UploadFile) -> bytes:
    if file.content_type not in ALLOWED_WAV_CONTENT_TYPES:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="file must be a WAV upload",
        )

    data = await file.read()
    validate_wav_bytes(data)

    return data


def validate_wav_bytes(data: bytes) -> None:
    if not data:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="WAV file is empty")
    if len(data) > MAX_WAV_BYTES:
        raise HTTPException(status_code=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE, detail="WAV file is too large")
    if len(data) < 44 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="invalid WAV header")

    try:
        with wave.open(io.BytesIO(data), "rb") as wav_file:
            if wav_file.getnchannels() != 1:
                raise ValueError("WAV must be mono")
            if wav_file.getframerate() != 16000:
                raise ValueError("WAV sample rate must be 16000 Hz")
            if wav_file.getsampwidth() != 2:
                raise ValueError("WAV sample width must be 16-bit PCM")
            if wav_file.getnframes() <= 0:
                raise ValueError("missing audio frames")
    except (wave.Error, EOFError, ValueError) as exc:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail=f"invalid WAV file: {exc}") from exc
