FROM python:3.12-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY services/laguna-daemon /app/laguna-daemon
RUN pip install --no-cache-dir -e /app/laguna-daemon

ENV SYNTH_LAGUNA_HOST=0.0.0.0 \
    SYNTH_LAGUNA_PORT=7333 \
    SYNTH_LAGUNA_BACKEND=mock \
    SYNTH_LAGUNA_DATA_DIR=/data/laguna \
    SYNTH_LAGUNA_MODELS_DIR=/models \
    PYTHONUNBUFFERED=1

EXPOSE 7333
VOLUME ["/data/laguna", "/models"]

CMD ["python", "-m", "laguna_daemon", "--host", "0.0.0.0", "--port", "7333"]
