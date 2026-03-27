FROM rust as build

WORKDIR /app

RUN apt update && apt install -y nodejs build-essential clang

RUN apt-get clean && \
    rm -rf /var/lib/apt/lists/*

COPY src/. ./src
COPY external/. ./external
COPY Cargo* ./

WORKDIR /app

RUN cargo build -r

FROM texlive/texlive:latest-full as runtime

RUN apt update && apt install -y exiftool
RUN apt-get clean && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /cleaner

COPY --from=build /app/external/pdfium ./pdfium
COPY --from=build /app/target/release/alc-ng ./bin/.

COPY tests/appendix /examples/appendix
COPY tests/1806_01078v1 /examples/1806_01078v1

ENV PDFIUM_PATH=/cleaner/pdfium

WORKDIR /cleaner/bin
ENTRYPOINT [ "./alc-ng" ]
CMD ["-f", "/input", "/output" ]
