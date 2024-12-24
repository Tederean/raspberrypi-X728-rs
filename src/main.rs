use chrono::Local;
use clap::Parser;
use decimal_percentage::Percentage;
use measurements::{Current, Voltage};
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};
use rppal::i2c::I2c;
use simple_signal::{self, Signal};
use thiserror::Error;
use tokio::process::Command;
use tokio::select;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};
use tokio_util::sync::CancellationToken;

const I2C_IP5310_ADDR: u16 = 0x36;
const I2C_IP5310_VOLTAGE_COMMAND: u8 = 0x02;
const I2C_IP5310_CAPACITY_COMMAND: u8 = 0x04;
const I2C_IP5310_CURRENT_COMMAND: u8 = 0x14;

const GPIO_BUTTON: u8 = 5;
const GPIO_POWER_LOSS: u8 = 6;
const GPIO_SOFTWARE_ALIVE: u8 = 12;
const GPIO_BUZZER: u8 = 20;

/// USV X728 control software
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Command to execute to shut down the system
    #[arg(long)]
    shutdown: String,

    /// Command to execute to reboot the system
    #[arg(long)]
    reboot: String,

    /// Timeout in seconds after power loss to shut down system
    #[arg(long)]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let cancellation_token = setup_signals();
    let usv = Box::new(X728USV::new()?);

    let power_loss_routine = power_loss_routine(&usv, &args, cancellation_token.clone());
    let button_routine = button_routine(&usv, &args, cancellation_token.clone());

    power_loss_routine.await?;
    button_routine.await?;

    Ok(())
}

async fn power_loss_routine(
    usv: &Box<X728USV>,
    args: &Args,
    cancellation_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(power_loss_action) = usv
        .get_power_loss_action(
            Duration::from_secs(args.timeout),
            cancellation_token.clone(),
        )
        .await
    {
        cancellation_token.cancel();

        match power_loss_action {
            PowerLossAction::CapacityLow(capacity) => {
                println!(
                    "Critical capacity of {} reached! Shutting down...",
                    capacity
                );
            }
            PowerLossAction::Timeout(elapsed) => {
                println!(
                    "Downtime of {} seconds reached! Shutting down...",
                    elapsed.as_secs()
                );
            }
        }

        run_shell_command(args.shutdown.clone()).await?;
    }

    Ok(())
}

async fn button_routine(
    usv: &Box<X728USV>,
    args: &Args,
    cancellation_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(button_action) = usv.get_button_action(cancellation_token.clone()).await {
        cancellation_token.cancel();

        match button_action {
            ButtonAction::Reboot(elapsed) => {
                println!(
                    "Button pressed for {} ms. Rebooting the system.",
                    elapsed.as_millis()
                );

                run_shell_command(args.reboot.clone()).await?;
            }
            ButtonAction::Shutdown(elapsed) => {
                println!(
                    "Button pressed for {} ms. Shutting down the system.",
                    elapsed.as_millis()
                );

                run_shell_command(args.shutdown.clone()).await?;
            }
        }
    }

    Ok(())
}

async fn run_shell_command(command: String) -> Result<(), Box<dyn std::error::Error>> {
    let mut parts = command.split_whitespace();

    let command = parts.next().ok_or(CommandError::NoCommand)?;
    let arguments = parts.collect::<Vec<_>>();

    let mut process = Command::new(command).args(arguments).spawn()?;
    let exit_status = process.wait().await?;

    if let Some(code) = exit_status.code() {
        if code != 0 {
            return Err(Box::new(CommandError::CommandFailed { code }));
        }
    }

    Ok(())
}

fn setup_signals() -> CancellationToken {
    let cancellation_token = CancellationToken::new();

    // Sigint: Ctrl+C  Sigterm: Nice shutdown  Sigkill: Forced shutdown
    simple_signal::set_handler(&[Signal::Int, Signal::Term, Signal::Kill], {
        let cancellation_token_clone = cancellation_token.clone();
        move |_| {
            cancellation_token_clone.cancel();
        }
    });

    cancellation_token
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum CommandError {
    #[error("Command is empty or whitespace.")]
    NoCommand,

    #[error("Command returned with exit status {code}.")]
    CommandFailed { code: i32 },
}

#[derive(Debug)]
struct X728USV {
    gpio: Gpio,
    i2c: I2c,
    gpio_button: InputPin,
    gpio_power_loss: InputPin,
    gpio_buzzer: Mutex<OutputPin>,
    gpio_software_alive: OutputPin,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum PowerSource {
    PowerSupply,
    Battery,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum ButtonState {
    Pressed,
    Released,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum ButtonAction {
    Reboot(Duration),
    Shutdown(Duration),
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum PowerLossAction {
    CapacityLow(Percentage),
    Timeout(Duration),
}

impl X728USV {
    fn new() -> Result<X728USV, Box<dyn std::error::Error>> {
        let gpio = Gpio::new()?;
        let mut i2c = I2c::new()?;

        let gpio_buzzer = gpio.get(GPIO_BUZZER)?.into_output_low();
        let gpio_power_loss = gpio.get(GPIO_POWER_LOSS)?.into_input();
        let gpio_software_alive = gpio.get(GPIO_SOFTWARE_ALIVE)?.into_output_high();
        let gpio_button = gpio.get(GPIO_BUTTON)?.into_input();

        i2c.set_slave_address(I2C_IP5310_ADDR)?;

        Ok(X728USV {
            gpio,
            i2c,
            gpio_button,
            gpio_power_loss,
            gpio_buzzer: Mutex::new(gpio_buzzer),
            gpio_software_alive,
        })
    }

    fn get_button_state(&self) -> ButtonState {
        match self.gpio_button.read() {
            Level::Low => ButtonState::Released,
            Level::High => ButtonState::Pressed,
        }
    }

    fn get_power_source(&self) -> PowerSource {
        match self.gpio_power_loss.read() {
            Level::Low => PowerSource::PowerSupply,
            Level::High => PowerSource::Battery,
        }
    }

    fn get_voltage(&self) -> rppal::i2c::Result<Voltage> {
        let read = u16::from_be(self.i2c.smbus_read_word(I2C_IP5310_VOLTAGE_COMMAND)?);

        let milli_volts = f64::from(read) * 1.25 / 16.0;

        Ok(Voltage::from_millivolts(milli_volts))
    }

    fn get_current(&self) -> rppal::i2c::Result<Current> {
        let read = i16::from_be(self.i2c.smbus_read_word(I2C_IP5310_CURRENT_COMMAND)? as i16);

        let milli_amperes = f64::from(read);

        Ok(Current::from_milliamperes(milli_amperes))
    }

    fn get_capacity(&self) -> rppal::i2c::Result<Percentage> {
        let read = u16::from_be(self.i2c.smbus_read_word(I2C_IP5310_CAPACITY_COMMAND)?);

        let ratio = f64::from(read) / 25600.0f64;

        Ok(Percentage::from(ratio))
    }

    async fn get_power_loss_action(
        &self,
        shutdown_duration: Duration,
        cancellation_token: CancellationToken,
    ) -> Option<PowerLossAction> {
        let mut last_source = self.get_power_source();
        let mut state_changed = Instant::now();

        while !cancellation_token.is_cancelled() {
            let new_source = self.get_power_source();

            if new_source != last_source {
                last_source = new_source.clone();
                state_changed = Instant::now();

                match new_source {
                    PowerSource::PowerSupply => {
                        println!(
                            "Power Supply restored at {}",
                            Local::now().format("%d-%m-%Y %H:%M:%S")
                        );

                        self.beep(
                            Duration::from_millis(50),
                            Duration::from_millis(100),
                            2,
                            cancellation_token.clone(),
                        )
                        .await;
                    }
                    PowerSource::Battery => {
                        println!(
                            "Power Supply failed at {}",
                            Local::now().format("%d-%m-%Y %H:%M:%S")
                        );

                        self.beep(
                            Duration::from_millis(500),
                            Duration::from_millis(500),
                            3,
                            cancellation_token.clone(),
                        )
                        .await;
                    }
                }
            }

            if !cancellation_token.is_cancelled() && new_source == PowerSource::Battery {
                let elapsed = state_changed.elapsed();

                if elapsed > shutdown_duration {
                    return Some(PowerLossAction::Timeout(elapsed));
                }

                match self.get_capacity() {
                    Ok(new_capacity) => {
                        if new_capacity < Percentage::from(0.2f32) {
                            return Some(PowerLossAction::CapacityLow(new_capacity));
                        }
                    }
                    Err(err) => println!("Error while reading capacity: {}", err),
                }
            }

            select! {
                _ = cancellation_token.cancelled() => {}
                _ = sleep(Duration::from_millis(10000)) => {}
            }
        }

        None
    }

    async fn get_button_action(
        &self,
        cancellation_token: CancellationToken,
    ) -> Option<ButtonAction> {
        let sleep_duration = Duration::from_millis(50);

        while !cancellation_token.is_cancelled() {
            match self.get_button_state() {
                ButtonState::Released => {
                    select! {
                        _ = cancellation_token.cancelled() => {}
                        _ = sleep(sleep_duration) => {}
                    }
                }
                ButtonState::Pressed => {
                    self.beep(
                        Duration::from_millis(200),
                        Duration::from_millis(200),
                        1,
                        cancellation_token.clone(),
                    )
                    .await;

                    let pulse_start = Instant::now();

                    while !cancellation_token.is_cancelled()
                        && self.get_button_state() == ButtonState::Pressed
                    {
                        select! {
                            _ = cancellation_token.cancelled() => {}
                            _ = sleep(sleep_duration) => {}
                        }
                    }

                    if !cancellation_token.is_cancelled() {
                        let elapsed = pulse_start.elapsed();

                        if elapsed >= Duration::from_secs(2) {
                            return Some(ButtonAction::Reboot(elapsed));
                        }

                        return Some(ButtonAction::Shutdown(elapsed));
                    }
                }
            }
        }

        None
    }

    async fn beep(
        &self,
        high_duration: Duration,
        low_duration: Duration,
        count: u8,
        cancellation_token: CancellationToken,
    ) {
        let mut gpio_buzzer = self.gpio_buzzer.lock().await;

        for counter in 0..count {
            if cancellation_token.is_cancelled() {
                return;
            }

            gpio_buzzer.set_high();

            select! {
                _ = cancellation_token.cancelled() => {}
                _ = sleep(high_duration) => {}
            }

            gpio_buzzer.set_low();

            if (counter + 1u8) < count {
                select! {
                    _ = cancellation_token.cancelled() => {}
                    _ = sleep(low_duration) => {}
                }
            }
        }
    }
}

impl std::fmt::Display for ButtonAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::fmt::Display for PowerLossAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::fmt::Display for PowerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::fmt::Display for ButtonState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
