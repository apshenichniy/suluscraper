use std::{
    collections::HashSet,
    io::Cursor,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use clap::{Parser, ValueEnum};
use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressStyle};
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    // let args = Args {
    //     tries: 3,
    //     target: ParseTarget::Ulta,
    //     limit: Some(300),
    //     workers: 15,
    //     delay: 5,
    //     output: PathBuf::from("/Users/apshenichniy/Downloads/suluscraper"),
    // };

    let started = Instant::now();

    let mpb = MultiProgress::new();
    let pages_pb = mpb.add(ProgressBar::new_spinner());
    pages_pb.enable_steady_tick(Duration::from_millis(20));
    pages_pb.set_style(ProgressStyle::with_template("{prefix:.green} {msg:.yellow}").unwrap());
    pages_pb.set_prefix("Parsing search results:");

    let (links_sender, links_reciever) = async_channel::unbounded();

    // spawn task to browse pages and fetch product links
    tokio::spawn(async move {
        let downloader = Downloader::new(args.attempts);
        let parser = &args.target.new_parser();

        let mut current_page: Option<String> = Some(parser.start_page());

        let mut processed_pages = 0;
        let mut parsed_links = 0;
        let mut finished = false;

        while let Some(page) = current_page {
            pages_pb.set_message(format!(
                "ready {} pages, {} links. Fetching {}",
                processed_pages, parsed_links, page
            ));

            let body = downloader.get(page.as_str()).await;
            match body {
                Ok(body) => {
                    let (next_page, product_links) = parser.parse_search_results_page(body);
                    // push links to queue
                    for link in product_links {
                        match links_sender.send(link).await {
                            Ok(_) => {
                                parsed_links += 1;
                                if let Some(limit) = args.limit {
                                    if parsed_links >= limit {
                                        finished = true;
                                        break;
                                    }
                                }
                            }
                            Err(err) => pages_pb.set_message(format!(
                                "Jobs sender has encountered an error: {}",
                                err
                            )),
                        }
                    }
                    if !finished {
                        current_page = next_page;
                    } else {
                        current_page = None;
                    }
                }
                Err(err) => {
                    pages_pb.set_message(format!("Downloader has encountered an error: {}", err));
                    current_page = None;
                }
            };
            processed_pages += 1;

            if current_page.is_some() {
                pages_pb.set_message(format!(
                    "ready {} pages, {} links. Waiting {} secs",
                    processed_pages, parsed_links, args.delay
                ));
                thread::sleep(Duration::from_secs(args.delay));
            }
        }
        pages_pb.finish_with_message(format!(
            "ready {} pages, {} links. DONE",
            processed_pages, parsed_links
        ));
    });

    // construct output directories
    let photos_output = args
        .output
        .clone()
        .join(args.target.output_name())
        .join("photos");

    let json_output = args
        .output
        .clone()
        .join(args.target.output_name())
        .join("json");

    // create output directories
    std::fs::create_dir_all(photos_output.as_path())
        .expect("Unable to create photos output directory");
    std::fs::create_dir_all(json_output.as_path()).expect("Unable to create json output directory");

    let (result_sender, mut result_reciever) = mpsc::unbounded_channel();

    // spawn worker tasks
    for w in 0..args.workers {
        let pb = mpb.add(ProgressBar::new_spinner());
        pb.enable_steady_tick(Duration::from_millis(20));
        pb.set_style(ProgressStyle::with_template("{prefix:.cyan} {msg:}").unwrap());
        pb.set_prefix(format!("worker {}: ", w + 1));

        let downloader = Downloader::new(args.attempts);
        let parser = args.target.new_parser();
        let links_r = links_reciever.clone();
        let result_s = result_sender.clone();
        let photos_output = photos_output.clone();
        let json_output = json_output.clone();

        tokio::spawn(async move {
            pb.set_message("waiting...");
            loop {
                match links_r.recv().await {
                    Ok(link) => {
                        pb.set_message(format!("processing {}", &link));

                        match downloader.get(link.as_str()).await {
                            Ok(body) => {
                                let mut product_info = parser.parse_product_page(body, link);
                                let timestamp;
                                let product_id = match &product_info.sku {
                                    Some(sku) => sku.as_str(),
                                    None => {
                                        timestamp = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap()
                                            .as_millis()
                                            .to_string();

                                        timestamp.as_str()
                                    }
                                };

                                let mut local_photos = vec![];
                                // download images
                                for (i, link) in product_info.photos.iter().enumerate() {
                                    let file_name = format!("{}_{}.png", product_id, i);
                                    let path = photos_output.join(file_name.as_str());
                                    pb.set_message(format!("downloading image {}", link));
                                    match downloader.get_file(link, path).await {
                                        Ok(()) => {
                                            local_photos.push(file_name);
                                        }
                                        Err(e) => {
                                            pb.set_message(format!(
                                                "Error encountered during image downloading: {}",
                                                e
                                            ));
                                        }
                                    }
                                }
                                product_info.photos = local_photos;

                                // save json
                                let json_name = format!("{}.json", product_id);
                                let json_path = json_output.join(json_name.as_str());
                                pb.set_message(format!("writing {}", json_name));
                                serde_json::to_writer_pretty(
                                    &std::fs::File::create(json_path).unwrap(),
                                    &product_info,
                                )
                                .unwrap();

                                result_s
                                    .send(WorkerResult {
                                        success: 1,
                                        images: product_info.photos.len(),
                                        error: 0,
                                    })
                                    .unwrap()
                            }
                            Err(_) => result_s
                                .send(WorkerResult {
                                    success: 0,
                                    images: 0,
                                    error: 1,
                                })
                                .unwrap(),
                        }
                        pb.set_message(format!("wating {} secs", args.delay));
                        thread::sleep(Duration::from_secs(args.delay));
                    }
                    Err(_) => break,
                }
            }
            pb.finish_with_message("waiting...");
        });
    }

    drop(result_sender);

    // process result channel
    let total_pb = mpb.add(ProgressBar::new_spinner());
    total_pb.enable_steady_tick(Duration::from_millis(20));
    total_pb.set_style(ProgressStyle::with_template("{prefix:.green} {msg:.yellow}").unwrap());
    total_pb.set_prefix("Total parsed:");

    let (mut success, mut images, mut error) = (0, 0, 0);

    loop {
        match result_reciever.recv().await {
            Some(result) => {
                success += result.success;
                images += result.images;
                error += result.error;
                total_pb.set_message(format!(
                    "success: {}, images: {}, error: {}",
                    success, images, error
                ));
            }
            None => {
                total_pb.finish_with_message(format!(
                    "success: {}, images: {}, error: {}. DONE",
                    success, images, error
                ));
                break;
            }
        };
    }
    //mpb.clear().unwrap();
    println!("DONE in {}", HumanDuration(started.elapsed()));
}

#[derive(Debug)]
struct WorkerResult {
    success: usize,
    images: usize,
    error: usize,
}

#[derive(Clone, Debug)]
struct Downloader {
    client: reqwest::Client,
    tries: usize,
}

impl Downloader {
    fn new(tries: usize) -> Self {
        Downloader {
            client: reqwest::Client::new(),
            tries,
        }
    }

    async fn get(&self, url: &str) -> Result<String, reqwest::Error> {
        let mut error: Option<reqwest::Error> = None;

        for _ in 0..self.tries {
            match self.client.get(url).send().await {
                Ok(response) => return response.text().await,
                Err(err) => error = Some(err),
            }
        }
        Err(error.unwrap())
    }

    async fn get_file(&self, url: &str, file_name: PathBuf) -> anyhow::Result<()> {
        let response = reqwest::get(url).await?;
        let mut file = std::fs::File::create(file_name)?;
        let mut content = Cursor::new(response.bytes().await?);
        std::io::copy(&mut content, &mut file)?;

        Ok(())
    }
}
trait StoreParser
where
    Self: Send + Sync,
{
    fn base_url(&self) -> &str;
    fn start_page(&self) -> String;
    fn parse_search_results_page(&self, body: String) -> (Option<String>, Vec<String>);
    fn parse_product_page(&self, body: String, link: String) -> ProductInfo;
}

struct UltaParser {
    start_page: &'static str,
    base_url: &'static str,
}

impl UltaParser {
    fn new() -> Self {
        UltaParser {
            start_page: "https://www.ulta.com/skin-care?N=1z12lx1Z2707&Ns=product.bestseller%7C1",
            base_url: "https://www.ulta.com",
        }
    }

    fn get_key_ingredients(&self, document: &Html) -> Option<String> {
        let details_selector =
            Selector::parse("details[aria-controls='Details'] > div > div").ok()?;
        let details_element = document.select(&details_selector).next()?;
        let mut details_children = details_element.children();

        while let Some(node) = details_children.next() {
            let element = ElementRef::wrap(node)?;
            if element.value().name() == "h4" && element.text().next()? == "Key Ingredients" {
                let next_ul = details_children.next()?;
                let selector = Selector::parse("li").ok()?;
                let text = ElementRef::wrap(next_ul)?
                    .select(&selector)
                    .map(|item| item.text().next().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n");

                return Some(text);
            }
        }

        None
    }
}

impl StoreParser for UltaParser {
    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn start_page(&self) -> String {
        String::from(self.start_page)
    }

    fn parse_search_results_page(&self, body: String) -> (Option<String>, Vec<String>) {
        let document = Html::parse_document(body.as_str());
        let next_page_selector = Selector::parse("a.next").unwrap();

        let next_page = match document.select(&next_page_selector).next() {
            Some(element) => match element.value().attr("href") {
                Some(href) => Some([&self.base_url, href].concat()),
                None => None,
            },
            None => None,
        };

        let products_selector = Selector::parse("div.productQvContainer").unwrap();
        let product_elements = document.select(&products_selector).collect::<Vec<_>>();
        let mut links = vec![];

        for el in product_elements {
            let link_selector = Selector::parse("a[href]").unwrap();
            if let Some(link_el) = el.select(&link_selector).next() {
                if let Some(href) = link_el.value().attr("href") {
                    links.push([&self.base_url, href].concat());
                }
            }
        }

        (next_page, links)
    }

    fn parse_product_page(&self, body: String, link: String) -> ProductInfo {
        let document = Html::parse_document(body.as_str());

        let brand = get_element_text(
            "div.ProductInformation > h1 > span > a.Link_Huge",
            &document,
        );
        let title = get_element_text(
            "div.ProductInformation > h1 > span.Text-ds--title-5",
            &document,
        );
        let description = get_element_text("div.ProductSummary > p", &document);
        let how_to_use = get_element_text(
            "details[aria-controls='How_To_Use'] > div.Accordion_Huge__content > div> p",
            &document,
        );
        let ingredients = get_element_text("details[aria-controls='Ingredients'] > div.Accordion_Huge__content > div.Markdown--body-2 > p", &document);
        let volume = get_element_text("div.ProductDimension > span.Text-ds--black", &document);
        let product_components = self.get_key_ingredients(&document);

        let mut photos = Vec::new();
        let mut sku = None;

        let state = get_element_text("script[id='apollo_state']", &document);
        match state {
            Some(state) => {
                let values = get_value_by_key(state.as_str(), "imageUrl")
                    .iter()
                    .map(|v| v.replace("\\u002F", "/"))
                    .map(|v| Url::parse(v.as_str()))
                    .filter_map(|v| v.ok())
                    .map(|v| v.as_str().to_owned())
                    .collect::<Vec<_>>();

                let hs: HashSet<String> = HashSet::from_iter(values);
                photos = hs.into_iter().collect::<Vec<_>>();

                sku = get_value_by_key(state.as_str(), "skuId").into_iter().next();
            }
            None => println!("none"),
        }

        ProductInfo {
            sku,
            brand,
            title,
            description,
            how_to_use,
            ingredients,
            volume,
            product_components,
            product_link: link,
            photos,
        }
    }
}

fn get_element_text(path: &str, document: &Html) -> Option<String> {
    let selector = Selector::parse(path).unwrap();
    let value = document
        .select(&selector)
        .map(|element| element.text().next().unwrap_or(""))
        .collect::<Vec<&str>>()
        .join("\n");

    if value.is_empty() {
        return None;
    }
    Some(value)
}

fn get_value_by_key(src: &str, key: &str) -> Vec<String> {
    let mut res = vec![];

    for (index, _) in src.match_indices(key) {
        let mut buf = vec![];
        let mut value_started = false;

        for ch in src[index + key.len() + 1..].chars() {
            match ch {
                '"' => {
                    if value_started {
                        break;
                    } else {
                        value_started = true;
                    }
                }
                _ => {
                    if value_started {
                        buf.push(ch);
                    }
                }
            }
        }
        res.push(buf.into_iter().collect::<_>());
    }

    res
}

#[allow(dead_code)]
#[derive(Serialize)]
struct ProductInfo {
    sku: Option<String>,
    brand: Option<String>,
    title: Option<String>,
    description: Option<String>,
    how_to_use: Option<String>,
    ingredients: Option<String>,
    volume: Option<String>,
    product_components: Option<String>,
    product_link: String,
    photos: Vec<String>,
}

#[derive(Parser, Debug)]
#[command(author = "Alexander Pshenichniy (alexander@sulu.beauty)")]
#[command(version = "0.1")]
#[command(
    about = "CLI program to scrape skin care products information. Currently only from https://www.ulta.com"
)]
struct Args {
    /// Target to scrape
    #[arg(short, long)]
    target: ParseTarget,

    /// Output directory
    #[arg(short, long)]
    output: PathBuf,

    /// Limit of products to parse, optional
    #[arg(short, long)]
    limit: Option<usize>,

    /// Num of workers
    #[arg(short, long, default_value_t = 5)]
    workers: usize,

    /// Number of request attempts
    #[arg(short, long, default_value_t = 3)]
    attempts: usize,

    /// Delay between requests, secs
    #[arg(short, long, default_value_t = 3)]
    delay: u64,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ParseTarget {
    Ulta,
}

impl ParseTarget {
    fn new_parser(&self) -> Box<dyn StoreParser> {
        match &self {
            ParseTarget::Ulta => Box::new(UltaParser::new()),
        }
    }

    fn output_name(&self) -> &'static str {
        match &self {
            ParseTarget::Ulta => "ulta",
        }
    }
}
