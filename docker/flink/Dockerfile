FROM eclipse-temurin:21-jre-jammy AS base

ARG FLINK_VERSION=2.0.0
ARG SCALA_VERSION=2.13

ENV FLINK_HOME=/opt/flink
ENV PATH="${FLINK_HOME}/bin:${PATH}"

# Install tini for proper PID 1 handling
RUN apt-get update && \
    apt-get install -y --no-install-recommends tini procps && \
    rm -rf /var/lib/apt/lists/*

# Download and extract Flink
RUN curl -fsSL "https://archive.apache.org/dist/flink/flink-${FLINK_VERSION}/flink-${FLINK_VERSION}-bin-scala_${SCALA_VERSION}.tgz" \
    | tar -xz -C /opt && \
    mv "/opt/flink-${FLINK_VERSION}" "${FLINK_HOME}"

# Remove unnecessary components to reduce image size
RUN rm -rf ${FLINK_HOME}/examples \
    ${FLINK_HOME}/opt/flink-python* \
    ${FLINK_HOME}/opt/flink-walkthrough*

# Create flink user
RUN groupadd -r flink && useradd -r -g flink -d ${FLINK_HOME} flink && \
    chown -R flink:flink ${FLINK_HOME}

WORKDIR ${FLINK_HOME}
USER flink

ENTRYPOINT ["tini", "--"]
