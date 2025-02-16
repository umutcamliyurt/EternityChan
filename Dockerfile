# Use Alpine Linux as base image
FROM alpine:latest

# Install dependencies
RUN apk update && \
    apk add --no-cache \
    curl \
    build-base \
    git \
    rust \
    tor \
    bash

# Install Rust (via rustup)
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

# Set environment variables for Rust installation
ENV PATH="/root/.cargo/bin:${PATH}"

# Clone the repository
RUN git clone https://github.com/umutcamliyurt/EternityChan.git

# Change to the project directory
WORKDIR /EternityChan

# Build the project
RUN cargo build --release

# Configure Tor for the hidden service
RUN mkdir -p /etc/tor && \
    echo "HiddenServiceDir /var/lib/tor/hidden_service/" > /etc/tor/torrc && \
    echo "HiddenServicePort 80 127.0.0.1:8000" >> /etc/tor/torrc

# Expose the required port for the hidden service
EXPOSE 8000

# Start Tor and the project
CMD tor & cargo run --release
