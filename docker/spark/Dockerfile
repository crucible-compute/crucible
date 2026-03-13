FROM eclipse-temurin:21-jre-jammy AS base

ARG SPARK_VERSION=4.0.0
ARG SCALA_VERSION=2.13
ARG HADOOP_VERSION=3

ENV SPARK_HOME=/opt/spark
ENV PATH="${SPARK_HOME}/bin:${SPARK_HOME}/sbin:${PATH}"

# Install tini for proper PID 1 handling
RUN apt-get update && \
    apt-get install -y --no-install-recommends tini procps && \
    rm -rf /var/lib/apt/lists/*

# Download and extract Spark
RUN curl -fsSL "https://archive.apache.org/dist/spark/spark-${SPARK_VERSION}/spark-${SPARK_VERSION}-bin-hadoop${HADOOP_VERSION}-scala${SCALA_VERSION}.tgz" \
    | tar -xz -C /opt && \
    mv "/opt/spark-${SPARK_VERSION}-bin-hadoop${HADOOP_VERSION}-scala${SCALA_VERSION}" "${SPARK_HOME}"

# Remove unnecessary JARs to reduce image size
RUN rm -rf ${SPARK_HOME}/examples \
    ${SPARK_HOME}/data \
    ${SPARK_HOME}/R \
    ${SPARK_HOME}/python/test_support \
    ${SPARK_HOME}/jars/hadoop-aliyun-*.jar \
    ${SPARK_HOME}/jars/hadoop-huaweicloud-*.jar \
    ${SPARK_HOME}/jars/hive-*.jar \
    ${SPARK_HOME}/jars/orc-*.jar \
    ${SPARK_HOME}/jars/parquet-cli-*.jar

# Create spark user
RUN groupadd -r spark && useradd -r -g spark -d ${SPARK_HOME} spark && \
    chown -R spark:spark ${SPARK_HOME}

WORKDIR ${SPARK_HOME}
USER spark

ENTRYPOINT ["tini", "--"]
