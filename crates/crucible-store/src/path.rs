/// Build object store paths for job artifacts following the bucket layout convention.
pub struct PathBuilder {
    bucket: String,
}

impl PathBuilder {
    pub fn new(bucket: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
        }
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn spark_driver_log(&self, job_id: &str) -> String {
        format!("logs/spark/{job_id}/driver.log")
    }

    pub fn spark_executor_log(&self, job_id: &str, executor_id: &str) -> String {
        format!("logs/spark/{job_id}/executor-{executor_id}.log")
    }

    pub fn spark_event_log(&self, job_id: &str) -> String {
        format!("spark-events/{job_id}")
    }

    pub fn flink_jm_log(&self, job_id: &str) -> String {
        format!("logs/flink/{job_id}/jobmanager.log")
    }

    pub fn flink_tm_log(&self, job_id: &str, tm_index: u32) -> String {
        format!("logs/flink/{job_id}/taskmanager-{tm_index}.log")
    }

    pub fn flink_checkpoint(&self, job_id: &str) -> String {
        format!("flink-checkpoints/{job_id}")
    }

    pub fn flink_savepoint(&self, job_id: &str, savepoint_id: &str) -> String {
        format!("flink-savepoints/{job_id}/{savepoint_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_paths() {
        let pb = PathBuilder::new("crucible-data");
        assert_eq!(pb.bucket(), "crucible-data");
        assert_eq!(
            pb.spark_driver_log("job-123"),
            "logs/spark/job-123/driver.log"
        );
        assert_eq!(
            pb.spark_executor_log("job-123", "0"),
            "logs/spark/job-123/executor-0.log"
        );
        assert_eq!(pb.spark_event_log("job-123"), "spark-events/job-123");
    }

    #[test]
    fn flink_paths() {
        let pb = PathBuilder::new("crucible-data");
        assert_eq!(
            pb.flink_jm_log("job-456"),
            "logs/flink/job-456/jobmanager.log"
        );
        assert_eq!(
            pb.flink_tm_log("job-456", 2),
            "logs/flink/job-456/taskmanager-2.log"
        );
        assert_eq!(pb.flink_checkpoint("job-456"), "flink-checkpoints/job-456");
        assert_eq!(
            pb.flink_savepoint("job-456", "sp-1"),
            "flink-savepoints/job-456/sp-1"
        );
    }
}
