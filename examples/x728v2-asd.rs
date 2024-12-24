use rpipal::i2c::{I2c, I2cOptions};
use std::time::Duration;
use tokio::task;
use tokio::time::sleep;

const GPIO_PORT: u8 = 26;
const I2C_ADDR: u16 = 0x36;

#[tokio::main]
async fn main() {
    let mut i2c = I2c::new(I2cOptions::new().device("/dev/i2c-1")).unwrap();
    let mut gpio = rpipal::gpio::Gpio::new().unwrap();
    gpio.set_mode(GPIO_PORT, rpipal::gpio::Mode::Output);

    loop {
        println!("******************");
        let voltage = read_voltage(&mut i2c).await;
        let capacity = read_capacity(&mut i2c).await;

        println!("Voltage:{:.2}V", voltage);
        println!("Battery:{}%", capacity);

        if capacity == 100.0 {
            println!("Battery FULL");
        }

        if capacity < 20.0 {
            println!("Battery Low");
        }

        if voltage < 3.00 {
            println!("Battery LOW!!!");
            println!("Shutdown in 10 seconds");

            let gpio_clone = gpio.clone();
            task::spawn(async move {
                sleep(Duration::from_secs(10)).await;
                gpio_clone.set_value(GPIO_PORT, rpipal::gpio::Value::High);
                sleep(Duration::from_secs(3)).await;
                gpio_clone.set_value(GPIO_PORT, rpipal::gpio::Value::Low);
            })
            .await;
        }

        sleep(Duration::from_secs(2)).await;
    }
}

async fn read_voltage(i2c: &mut I2c) -> f32 {
    let address = I2C_ADDR;
    let read = i2c.read_word_data(address, 2).unwrap();
    let swapped = u16::from_le(read);
    let voltage = f32::from(swapped) * 1.25 / 1000.0 / 16.0;
    voltage
}

async fn read_capacity(i2c: &mut I2c) -> f32 {
    let address = I2C_ADDR;
    let read = i2c.read_word_data(address, 4).unwrap();
    let swapped = u16::from_le(read);
    let capacity = f32::from(swapped) / 256.0;
    capacity
}
