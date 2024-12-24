// X728 AC Power loss / power adapter failture detection

use rpipal::gpio::{Gpio, Mode, Value};
use std::time::Duration;
use tokio::time::sleep;

const PLD_PIN: u8 = 6;
const BUZZER_PIN: u8 = 20;

#[tokio::main]
async fn main() {
    let mut gpio = Gpio::new().unwrap();
    gpio.set_mode(PLD_PIN, Mode::Input);
    gpio.set_mode(BUZZER_PIN, Mode::Output);

    loop {
        let i = gpio.read(PLD_PIN).unwrap();
        if i == Value::Low {
            println!("AC Power OK");
            gpio.write(BUZZER_PIN, Value::Low);
        } else if i == Value::High {
            println!("Power Supply A/C Lost");
            gpio.write(BUZZER_PIN, Value::High);
            sleep(Duration::from_millis(100)).await;
            gpio.write(BUZZER_PIN, Value::Low);
            sleep(Duration::from_millis(100)).await;
        }
        sleep(Duration::from_secs(1)).await;
    }
}
