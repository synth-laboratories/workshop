FROM python:3.12-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY services/local-runtime /app/local-runtime
ENV PYTHONPATH=/app/local-runtime/src \
    SYNTH_RUNTIME_HOST=0.0.0.0 \
    SYNTH_RUNTIME_PORT=8765 \
    SYNTH_RUNTIME_DATA_DIR=/data/runtime \
    PYTHONUNBUFFERED=1

EXPOSE 8765
VOLUME ["/data/runtime"]

CMD ["python", "-m", "synth_local_runtime", "--host", "0.0.0.0", "--port", "8765"]
