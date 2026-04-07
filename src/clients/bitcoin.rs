use electrum_client::{
    Client, ElectrumApi, bitcoin::{Network, Txid, address::Address, bip32::{DerivationPath, Xpub}, secp256k1::Secp256k1}
};

use serde::Deserialize;
use chrono::{NaiveDate, Utc};
use std::{collections::{HashMap, HashSet}, f64, str::FromStr};

#[derive(Deserialize)]
struct Prices {
    #[serde(rename = "USD")]
    usd: f64,
    #[serde(rename = "EUR")]
    eur: f64,
}

pub enum AddressType {
    P2PKH,
    P2SH_P2WPKH,
    P2WPKH,
    P2TR,
}

pub enum FiatCurrency {
    USD,
    EUR,
}

const GAP_LIMIT: usize = 20;

#[derive(Debug, Clone)]
pub enum TxType {
    Incoming,
    Outgoing,
    Fee,
    Interest,
    Other,
}

#[derive(Debug, Clone)]
pub struct TxRecord {
    pub txid: String,
    pub date: NaiveDate,
    pub amount_sats: i64,   // positive = received, negative = sent
    pub tx_type: TxType,
}

#[derive(Debug)]
pub struct BitcoinBalance{
    pub confirmed: u64,
    pub unconfirmed: i64,
}

impl BitcoinBalance{
    fn new(confirmed: u64, unconfirmed: i64) -> Self {
        BitcoinBalance{
            confirmed: confirmed, 
            unconfirmed: unconfirmed,
        }
    }
}


pub struct BitcoinAccount {
    pub name: String,
    pub pubkey: String,
    pub address_type: AddressType,
    pub client: Client, // Electrum
    pub fiat: FiatCurrency
}

impl BitcoinAccount {

    fn fiat_code(&self) -> &'static str {
        match self.fiat {
            FiatCurrency::USD => "USD",
            FiatCurrency::EUR => "EUR",
        }
    }

    pub fn new(
        name: String,
        pubkey: String,
        address_type: AddressType,
        client: Client,
        fiat: FiatCurrency,
    ) -> Self {
        Self {
            name,
            pubkey,
            address_type,
            client,
            fiat,
        }
    }

    pub async fn get_balance(&self) -> Result<BitcoinBalance, Box<dyn std::error::Error>> {
        let xpub: Xpub = self.pubkey.parse()?;
        let secp = Secp256k1::new();

        let mut total_unconfirmed: i64 = 0;
        let mut total_confirmed: u64 = 0;

        for change in 0..=1 {
            let mut i = 0;
            let mut empties = 0;

            while empties < 20 {
                let path = DerivationPath::from_str(&format!("m/{}/{}", change, i))?;
                let child = xpub.derive_pub(&secp, &path)?;
                let pubkey = child.to_pub();

                let addr = match self.address_type {
                    AddressType::P2WPKH => {
                        Address::p2wpkh(&pubkey, Network::Bitcoin)
                    }
                    AddressType::P2PKH => {
                        Address::p2pkh(&pubkey, Network::Bitcoin)
                    }
                    AddressType::P2SH_P2WPKH => {
                        Address::p2shwpkh(&pubkey, Network::Bitcoin)
                    }
                    AddressType::P2TR => {
                        // Taproot needs x-only pubkey
                        unimplemented!()
                        //let x_only = pubkey.inner.x_only_public_key().0;
                        //Address::p2tr(&secp, x_only, None, Network::Bitcoin)
                    }
                };

                let script = addr.script_pubkey();
                let balance = self.client.script_get_balance(&script)?;

                let total = balance.confirmed as i64 + balance.unconfirmed;

                if total == 0 {
                    empties += 1;
                } else {
                    empties = 0;
                    total_confirmed += balance.confirmed;
                    total_unconfirmed += balance.unconfirmed;
                }

                i += 1;
            }
        }

        Ok(BitcoinBalance::new(total_confirmed, total_unconfirmed))
    }

    pub async fn get_price(&self) -> Result<f64, Box<dyn std::error::Error>> {
        let uri = "https://mempool.space/api/v1/prices";
        let resp = reqwest::get(uri).await?
            .json::<Prices>().await?;

        let price = match self.fiat {
            FiatCurrency::USD => resp.usd,
            FiatCurrency::EUR => resp.eur,
        };

        Ok(price)
    }

    /// Fetch daily BTC price history using the mempool.space historical-price API.
    /// Requests are batched in chunks of 20 concurrent fetches to avoid rate-limiting.
    pub async fn get_daily_prices(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<HashMap<NaiveDate, f64>, Box<dyn std::error::Error>> {
        if start > end {
            return Ok(HashMap::new());
        }

        let currency = self.fiat_code(); // "USD" or "EUR"
        let http = reqwest::Client::new();

        let mut dates = Vec::new();
        let mut day = start;
        while day <= end {
            dates.push(day);
            day = match day.succ_opt() {
                Some(next) => next,
                None => break,
            };
        }

        let mut raw_prices: HashMap<NaiveDate, f64> = HashMap::new();

        // Fetch in chunks of 20 concurrent requests
        for chunk in dates.chunks(20) {
            let mut set = tokio::task::JoinSet::new();

            for &date in chunk {
                let ts = date
                    .and_hms_opt(12, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                let uri = format!(
                    "https://mempool.space/api/v1/historical-price?currency={}&timestamp={}",
                    currency, ts
                );
                let c = http.clone();
                set.spawn(async move {
                    let resp = c
                        .get(&uri)
                        .send()
                        .await?
                        .json::<serde_json::Value>()
                        .await?;
                    Ok::<(NaiveDate, serde_json::Value), reqwest::Error>((date, resp))
                });
            }

            while let Some(result) = set.join_next().await {
                if let Ok(Ok((date, json))) = result {
                    if let Some(arr) = json.get("prices").and_then(|v| v.as_array()) {
                        if let Some(entry) = arr.first() {
                            if let Some(price) = entry.get(currency).and_then(|v| v.as_f64()) {
                                raw_prices.insert(date, price);
                            }
                        }
                    }
                }
            }
        }

        // Fill forward so every day in the range has a price
        let mut filled_prices: HashMap<NaiveDate, f64> = HashMap::new();
        let mut last_price: Option<f64> = None;
        let mut day = start;
        while day <= end {
            if let Some(&price) = raw_prices.get(&day) {
                last_price = Some(price);
            }
            if let Some(price) = last_price {
                filled_prices.insert(day, price);
            }
            day = match day.succ_opt() {
                Some(next) => next,
                None => break,
            };
        }

        Ok(filled_prices)
    }

    /// Scan XPUB and return all transactions using batch history requests
    pub fn scan_xpub_history(&self, skip_unconfirmed: bool) -> Result<Vec<TxRecord>, Box<dyn std::error::Error>> {
        let xpub: Xpub = self.pubkey.parse()?;
        let secp = Secp256k1::new();
        let mut tx_heights: HashMap<Txid, i32> = HashMap::new();
        let mut wallet_scripts = HashSet::new();

        // iterate both chains: 0 = external, 1 = internal
        for change in 0..=1 {
            let mut index = 0;
            let mut empties = 0;

            while empties < GAP_LIMIT {
                let mut batch_scripts = Vec::with_capacity(GAP_LIMIT);

                for _ in 0..GAP_LIMIT {
                    let path = DerivationPath::from_str(&format!("m/{}/{}", change, index))?;
                    let child = xpub.derive_pub(&secp, &path)?;
                    let pubkey = child.to_pub();

                    let addr = match self.address_type {
                        AddressType::P2WPKH => Address::p2wpkh(&pubkey, Network::Bitcoin),
                        AddressType::P2PKH => Address::p2pkh(&pubkey, Network::Bitcoin),
                        AddressType::P2SH_P2WPKH => Address::p2shwpkh(&pubkey, Network::Bitcoin),
                        AddressType::P2TR => {
                            unimplemented!();
                            //let x_only = pubkey.inner.x_only_public_key().0;
                            //Address::p2tr(&secp, x_only, None, Network::Bitcoin)
                        }
                    };

                    let script = addr.script_pubkey();
                    wallet_scripts.insert(script.clone());
                    batch_scripts.push(script);
                    index += 1;
                }

                let script_refs: Vec<_> = batch_scripts.iter().map(|s| s.as_script()).collect();
                let histories = self.client.batch_script_get_history(script_refs)?;

                for script_history in histories {
                    if script_history.is_empty() {
                        empties += 1;
                        continue;
                    }

                    empties = 0;
                    for hist in script_history {
                        if skip_unconfirmed && hist.height <= 0 {
                            continue;
                        }

                        tx_heights
                            .entry(hist.tx_hash)
                            .and_modify(|h| *h = (*h).max(hist.height))
                            .or_insert(hist.height);
                    }
                }
            }
        }

        let mut header_cache: HashMap<i32, u32> = HashMap::new();
        let mut tx_cache: HashMap<Txid, electrum_client::bitcoin::Transaction> = HashMap::new();
        let mut all_records: Vec<TxRecord> = Vec::with_capacity(tx_heights.len());

        for (txid, height) in tx_heights {
            let tx = self.client.transaction_get(&txid)?;
            tx_cache.insert(txid, tx.clone());

            let mut received_sats: i64 = 0;
            for output in &tx.output {
                if wallet_scripts.contains(&output.script_pubkey) {
                    received_sats += output.value.to_sat() as i64;
                }
            }

            let mut sent_inputs_sats: i64 = 0;
            for input in &tx.input {
                let prevout = input.previous_output;
                if prevout.is_null() {
                    continue;
                }

                let prev_txid = prevout.txid;
                let prev_vout = prevout.vout as usize;

                if !tx_cache.contains_key(&prev_txid) {
                    let prev_tx = self.client.transaction_get(&prev_txid)?;
                    tx_cache.insert(prev_txid, prev_tx);
                }

                if let Some(prev_tx) = tx_cache.get(&prev_txid) {
                    if let Some(prev_output) = prev_tx.output.get(prev_vout) {
                        if wallet_scripts.contains(&prev_output.script_pubkey) {
                            sent_inputs_sats += prev_output.value.to_sat() as i64;
                        }
                    }
                }
            }

            let amount_sats = received_sats - sent_inputs_sats;
            let tx_type = if amount_sats > 0 {
                TxType::Incoming
            } else if amount_sats < 0 {
                TxType::Outgoing
            } else {
                TxType::Other
            };

            let date = if height > 0 {
                let ts = if let Some(cached) = header_cache.get(&height) {
                    *cached
                } else {
                    let header = self.client.block_header(height as usize)?;
                    let ts = header.time;
                    header_cache.insert(height, ts);
                    ts
                };

                chrono::DateTime::from_timestamp(ts as i64, 0)
                    .map(|dt| dt.date_naive())
                    .unwrap_or_else(|| Utc::now().date_naive())
            } else {
                Utc::now().date_naive()
            };

            all_records.push(TxRecord {
                txid: txid.to_string(),
                date,
                amount_sats,
                tx_type,
            });
        }

        all_records.sort_by(|a, b| a.date.cmp(&b.date).then(a.txid.cmp(&b.txid)));

        Ok(all_records)
   }
}
