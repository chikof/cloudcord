use async_recursion::async_recursion;
use nanoid::nanoid;
use reqwest::multipart;
use std::env::args;
use std::fs::File;
use std::io::{self, Read, Stdout, Write};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

#[derive(serde::Deserialize, Clone, Debug)]
pub struct RatelimitResponse {
	pub message: String,
	pub retry_after: u64,
	pub global: bool,
	pub code: i32,
}

fn divide_file(file_path: &str, buffer_size: usize) -> io::Result<Vec<Vec<u8>>> {
	let mut file = File::open(file_path)?;
	let file_size = file.metadata()?.len() as usize;
	let num_buffers = (file_size + buffer_size - 1) / buffer_size;

	let mut buffers = Vec::with_capacity(num_buffers);
	for _ in 0..num_buffers - 1 {
		let mut buffer = vec![0; buffer_size];
		file.read_exact(&mut buffer)?;
		buffers.push(buffer);
	}

	let remaining_size = file_size - (num_buffers - 1) * buffer_size;
	let mut last_buffer = vec![0; remaining_size];
	file.read_exact(&mut last_buffer)?;
	buffers.push(last_buffer);

	Ok(buffers)
}

// TODO: store the message links in a db
#[async_recursion]
async fn upload_buffer(
	buffer: Vec<u8>,
	id: String,
	stdout: &mut Stdout,
) -> Result<(), Box<dyn std::error::Error>> {
	let webhook_url = std::env::var("WEBHOOK_URL").expect("WEBHOOK_URL not set");
	let client = reqwest::Client::new();
	let buffer_to_upload = buffer.clone();

	let form_data = multipart::Form::new()
		.part(
			"files[]",
			multipart::Part::stream(buffer_to_upload).file_name("buffer.txt"),
		)
		.text("content", format!("ID: {id}"));

	// Send the files to Discord using a webhook
	let response = client
		.post(webhook_url)
		.multipart(form_data)
		.send()
		.await
		.expect("Failed to send request");

	// if we get ratelimited, wait the specified amount of time and retry
	if response.status() == 429 {
		let ratelimit_response = response
			.json::<RatelimitResponse>()
			.await
			.expect("Failed to parse ratelimit response");

		stdout
			.write_all(
				format!(
					"Ratelimited! Retrying in: {} seconds",
					ratelimit_response.retry_after,
				)
				.as_bytes(),
			)
			.unwrap();

		sleep(Duration::from_secs(ratelimit_response.retry_after)).await;
		upload_buffer(buffer, id, stdout).await?;
	}

	Ok(())
}

#[tokio::main]
async fn main() {
	dotenv::dotenv().ok();

	let args: Vec<String> = args().collect();
	let file_path = Path::new::<String>(&args[1]);
	let buffer_size = 24 * 1024 * 1024;

	let path = file_path.canonicalize().ok();
	let mut stdout = io::stdout();

	match divide_file(&path.unwrap().to_str().unwrap(), buffer_size) {
		Ok(buffers) => {
			let id = nanoid!();
			for (i, buffer) in buffers.iter().enumerate() {
				let _ = upload_buffer(buffer.clone(), id.clone(), &mut stdout).await;
				stdout
					.write_all(format!("Uploaded buffer: {}/{}\n", i + 1, buffers.len()).as_bytes())
					.unwrap();
				stdout.flush().unwrap();
			}
		}
		Err(err) => eprintln!("Error: {:?}", err),
	}
}
