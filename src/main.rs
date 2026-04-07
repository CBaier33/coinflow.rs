use chrono::{Datelike, Duration, Months, NaiveDate, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use coinflow::{AddressType, BitcoinAccount, FiatCurrency};
use csv::Writer;
use electrum_client::Client;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Parser)]
#[command(name = "coinflow")]
#[command(about = "XPUB wallet reporting CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    ExportCsv(ExportCsvArgs),
}

#[derive(Clone, ValueEnum)]
enum AddressTypeArg {
    P2pkh,
    P2shP2wpkh,
    P2wpkh,
}

#[derive(Clone, ValueEnum)]
enum FiatArg {
    Usd,
    Eur,
}

#[derive(Clone, ValueEnum)]
enum IntervalArg {
    Day,
    Week,
    Month,
}

#[derive(Clone, ValueEnum)]
enum FormatArg {
    Report,
    Actual,
}

#[derive(Args)]
struct ExportCsvArgs {
    #[arg(long, help = "Electrum server URL, e.g. ssl://electrum.blockstream.info:60002")]
    electrum_url: String,
    #[arg(long, help = "Extended public key (xpub/ypub/zpub) to scan")]
    xpub: String,
    #[arg(long, default_value = "wallet", help = "Account label used in reporting")]
    name: String,
    #[arg(
        long,
        value_enum,
        default_value = "p2wpkh",
        help = "Address derivation type for the provided extended public key"
    )]
    address_type: AddressTypeArg,
    #[arg(long, value_enum, default_value = "usd", help = "Fiat currency for valuation")]
    fiat: FiatArg,
    #[arg(long, default_value = "wallet_report.csv", help = "Output CSV file path")]
    output: String,
    #[arg(long, default_value_t = false, help = "Include unconfirmed transactions")]
    include_unconfirmed: bool,
    #[arg(long, value_enum, default_value = "day", help = "Time bucket size for report rows")]
    interval: IntervalArg,
    #[arg(long, value_enum, default_value = "report", help = "CSV layout to generate")]
    format: FormatArg,
}

#[derive(Serialize)]
struct CsvRow {
    date: String,
    txid: String,
    tx_kind: String,
    btc_delta: f64,
    btc_balance: f64,
    price_fiat: f64,
    fiat_delta: f64,
    interest_fiat: f64,
    invested_fiat: f64,
    account_value_fiat: f64,
    amount: f64,
    profit_fiat: f64,
    profit_delta: f64,  // change in profit contributed by this row
}

#[derive(Serialize)]
struct ActualCsvRow {
    #[serde(rename = "Date")]
    date: String,
    #[serde(rename = "Payee")]
    payee: String,
    #[serde(rename = "Notes")]
    notes: String,
    #[serde(rename = "Amount")]
    amount: f64,
}

impl From<AddressTypeArg> for AddressType {
    fn from(value: AddressTypeArg) -> Self {
        match value {
            AddressTypeArg::P2pkh => AddressType::P2PKH,
            AddressTypeArg::P2shP2wpkh => AddressType::P2SH_P2WPKH,
            AddressTypeArg::P2wpkh => AddressType::P2WPKH,
        }
    }
}

impl From<FiatArg> for FiatCurrency {
    fn from(value: FiatArg) -> Self {
        match value {
            FiatArg::Usd => FiatCurrency::USD,
            FiatArg::Eur => FiatCurrency::EUR,
        }
    }
}

fn sats_to_btc(sats: i64) -> f64 {
    sats as f64 / 100_000_000.0
}

fn format_actual_date(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|parsed| parsed.format("%m/%d/%Y").to_string())
        .unwrap_or_else(|_| date.to_string())
}

fn bucket_start(date: NaiveDate, interval: &IntervalArg) -> NaiveDate {
    match interval {
        IntervalArg::Day => date,
        IntervalArg::Week => {
            let offset = date.weekday().num_days_from_monday() as i64;
            date - Duration::days(offset)
        }
        IntervalArg::Month => date.with_day(1).unwrap_or(date),
    }
}

fn next_bucket_start(start: NaiveDate, interval: &IntervalArg) -> Option<NaiveDate> {
    match interval {
        IntervalArg::Day => start.succ_opt(),
        IntervalArg::Week => start.checked_add_signed(Duration::days(7)),
        IntervalArg::Month => start.checked_add_months(Months::new(1)),
    }
}

fn bucket_end(start: NaiveDate, interval: &IntervalArg, today: NaiveDate) -> NaiveDate {
    let end = next_bucket_start(start, interval)
        .and_then(|next| next.pred_opt())
        .unwrap_or(today);
    if end > today {
        today
    } else {
        end
    }
}

async fn export_csv(args: ExportCsvArgs) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(&args.electrum_url)?;
    let account = BitcoinAccount::new(
        args.name,
        args.xpub,
        args.address_type.into(),
        client,
        args.fiat.into(),
    );

    let txs = account.scan_xpub_history(!args.include_unconfirmed)?;
    eprintln!("[debug] scan found {} transactions", txs.len());

    let today = Utc::now().date_naive();
    let start_date = txs.first().map(|t| t.date).unwrap_or(today);

    let mut price_by_day = account.get_daily_prices(start_date, today).await?;
    eprintln!("[debug] daily prices fetched: {} days", price_by_day.len());
    if !price_by_day.contains_key(&today) {
        price_by_day.insert(today, account.get_price().await?);
    }

    let mut txs_by_day: HashMap<NaiveDate, Vec<_>> = HashMap::new();
    for tx in txs {
        let period = bucket_start(tx.date, &args.interval);
        txs_by_day.entry(period).or_default().push(tx);
    }

    let mut rows: Vec<CsvRow> = Vec::new();

    let mut day = bucket_start(start_date, &args.interval);
    let end_day = bucket_start(today, &args.interval);
    let mut prev_price: Option<f64> = None;
    let mut balance_sats: i64 = 0;
    let mut invested_fiat: f64 = 0.0;
    let mut prev_profit: f64 = 0.0;
    let mut prev_account_value: f64 = 0.0;

    while day <= end_day {

        let period_end = bucket_end(day, &args.interval, today);
        let interval_start_account_value = prev_account_value;
        let mut interval_transaction_amount = 0.0;

        // Prefer the period-end price, then carry the previous known price forward.
        let price_today: Option<f64> = price_by_day
            .get(&period_end)
            .copied()
            .or(prev_price);

        // Transaction rows: always recorded regardless of price availability.
        if let Some(day_txs) = txs_by_day.get_mut(&day) {
            day_txs.sort_by(|a, b| a.date.cmp(&b.date).then(a.txid.cmp(&b.txid)));

            for tx in day_txs.iter() {
                balance_sats += tx.amount_sats;
                let btc_delta = sats_to_btc(tx.amount_sats);
                let price = price_by_day
                    .get(&tx.date)
                    .copied()
                    .or(prev_price)
                    .or(price_today)
                    .unwrap_or(0.0);
                let fiat_delta = btc_delta * price;
                let amount = fiat_delta;
                invested_fiat += fiat_delta;
                interval_transaction_amount += amount;

                let account_value = sats_to_btc(balance_sats) * price;
                let profit_fiat = account_value - invested_fiat;
                rows.push(CsvRow {
                    date: period_end.to_string(),
                    txid: tx.txid.clone(),
                    tx_kind: format!("{:?}", tx.tx_type),
                    btc_delta,
                    btc_balance: sats_to_btc(balance_sats),
                    price_fiat: price,
                    fiat_delta,
                    interest_fiat: 0.0,
                    invested_fiat,
                    account_value_fiat: account_value,
                    amount,
                    profit_delta: profit_fiat - prev_profit,
                    profit_fiat,
                });
                prev_profit = profit_fiat;
            }
        }

        // Exactly one interest row per interval: residual move to the interval-end valuation.
        let interval_price = price_today.unwrap_or(0.0);
        let interval_account_value = sats_to_btc(balance_sats) * interval_price;
        let interest_amount = interval_account_value
            - interval_start_account_value
            - interval_transaction_amount;
        let profit_fiat = interval_account_value - invested_fiat;

        rows.push(CsvRow {
            date: period_end.to_string(),
            txid: String::new(),
            tx_kind: "Interest".to_string(),
            btc_delta: 0.0,
            btc_balance: sats_to_btc(balance_sats),
            price_fiat: interval_price,
            fiat_delta: 0.0,
            interest_fiat: interest_amount,
            invested_fiat,
            account_value_fiat: interval_account_value,
            amount: interest_amount,
            profit_delta: profit_fiat - prev_profit,
            profit_fiat,
        });

        prev_profit = profit_fiat;
        prev_account_value = interval_account_value;

        if let Some(price) = price_today {
            prev_price = Some(price);
        }
        day = match next_bucket_start(day, &args.interval) {
            Some(next) => next,
            None => break,
        };
    }

    let mut writer = Writer::from_path(&args.output)?;
    match args.format {
        FormatArg::Report => {
            for row in rows {
                writer.serialize(row)?;
            }
        }
        FormatArg::Actual => {
            for row in rows.into_iter().filter(|row| row.amount.abs() > 0.000_000_1) {
                let notes = if row.txid.is_empty() {
                    row.tx_kind.clone()
                } else {
                    format!("{}", row.txid)
                };

                writer.serialize(ActualCsvRow {
                    date: format_actual_date(&row.date),
                    payee: row.tx_kind,
                    notes,
                    amount: (row.amount * 100.0).round() / 100.0,
                })?;
            }
        }
    }
    writer.flush()?;

    let latest_price = prev_price.unwrap_or(account.get_price().await?);
    let current_value = sats_to_btc(balance_sats) * latest_price;

    println!("CSV written to {}", args.output);
    println!("Current value: {:.2}", current_value);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ExportCsv(args) => export_csv(args).await?,
    }

    Ok(())
}
