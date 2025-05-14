use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
    time::Duration,
};

use csv::{Writer, WriterBuilder};
use host_executor::ExecutionHooks;
use serde::{Deserialize, Serialize};
use zkm_sdk::{ExecutionReport, ZKMVerifyingKey};

#[derive(Serialize, Deserialize)]
struct ExecutionReportData {
    chain_id: u64,
    number_cycles: u64,
    proving_time: u64,
}

#[derive(Debug)]
pub struct PersistExecutionReport {
    chain_id: u64,
    report_path: PathBuf,
    precompile_tracking: bool,
    opcode_tracking: bool,
}

impl PersistExecutionReport {
    pub fn new(
        chain_id: u64,
        report_path: PathBuf,
        precompile_tracking: bool,
        opcode_tracking: bool,
    ) -> Self {
        Self { chain_id, report_path, precompile_tracking, opcode_tracking }
    }

    fn write_header(&self, writer: &mut Writer<File>) -> csv::Result<()> {
        let headers = vec![
            "block_number".to_string(),
            "proving_time".to_string(),
            "proving_cycles".to_string(),
        ];

        writer.write_record(&headers)
    }

    fn write_record(
        &self,
        writer: &mut Writer<File>,
        block_number: u64,
        execution_report: &ExecutionReport,
        proving_duration: Duration,
    ) -> csv::Result<()> {
        let record = vec![
            block_number.to_string(),
            execution_report.total_instruction_count().to_string(),
            proving_duration.as_secs_f32().to_string(),
        ];

        writer.write_record(&record)
    }
}

impl ExecutionHooks for PersistExecutionReport {
    async fn on_proving_end(
        &self,
        block_number: u64,
        _proof_bytes: &[u8],
        _vk: &ZKMVerifyingKey,
        execution_report: &ExecutionReport,
        proving_duration: Duration,
    ) -> eyre::Result<()> {
        println!("\nExecution report:\n{}", execution_report);

        // Open the file for appending or create it if it doesn't exist
        let file = OpenOptions::new().append(true).create(true).open(self.report_path.clone())?;

        // Check if the file is empty
        let file_is_empty = file.metadata()?.len() == 0;
        let mut writer = WriterBuilder::new().from_writer(file);

        if file_is_empty {
            self.write_header(&mut writer)?;
        }

        self.write_record(&mut writer, block_number, execution_report, proving_duration)?;

        writer.flush()?;

        Ok(())
    }
}
