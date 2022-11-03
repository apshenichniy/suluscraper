use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

use crate::args::ScrapeTarget;

#[derive(Debug)]
pub struct ProductShortDetails {
    pub id: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Url,
}

#[derive(Serialize, Debug)]
pub struct ProductDetails {
    pub target: ScrapeTarget,
    pub id: Option<String>,
    pub brand: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub price: Option<String>,
    pub rating: Option<String>,
    pub product_type: Option<String>,
    pub skin_issues: Option<String>,
    pub skin_type: Option<String>,
    pub product_components: Option<String>,
    pub ingredients: Option<String>,
    pub volume: Option<String>,
    pub product_link: String,
    pub how_to_use: Option<String>,
}

pub trait ProductParser
where
    Self: Send + Sync,
{
    fn get_total_number(&self, document: &Html) -> Option<usize>;
    fn get_first_page(&self) -> Url;
    fn get_next_page(&self, document: &Html) -> Option<Url>;
    fn get_product_short_details(&self, document: &Html) -> Vec<ProductShortDetails>;
    fn get_product_details(&self, document: &Html, url: &Url) -> ProductDetails;
}

#[derive(Debug)]
pub struct UltaParser {
    pub base_url: Url,
    pub start_url: &'static str,
}

impl UltaParser {
    pub fn new() -> Self {
        UltaParser {
            base_url: Url::parse("https://www.ulta.com").unwrap(),
            start_url: "/skin-care?N=1z12lx1Z2707&Ns=product.bestseller%7C1",
        }
    }
}

impl ProductParser for UltaParser {
    fn get_total_number(&self, document: &Html) -> Option<usize> {
        let selector = Selector::parse("span.search-res-number").unwrap();

        document
            .select(&selector)
            .next()
            .and_then(|element| element.text().next().and_then(|s| s.parse::<usize>().ok()))
    }

    fn get_product_short_details(&self, document: &Html) -> Vec<ProductShortDetails> {
        let products_selector = Selector::parse("div.productQvContainer").unwrap();
        let products_elements = document.select(&products_selector);
        let products: Vec<ElementRef> = products_elements.collect();

        let mut vec = vec![];
        for element in products {
            // extract product link
            let link_selector = Selector::parse("a[href]").unwrap();
            let url = match element.select(&link_selector).next() {
                Some(link_element) => link_element.value().attr("href"),
                None => None,
            }
            .and_then(|link| self.base_url.clone().join(link).ok());

            // if failed to extract url skip adding product
            let url = match url {
                Some(url) => url,
                None => continue,
            };

            // extract product id
            let id = element.value().attr("id").map(|s| String::from(s));

            // extract product title
            let title_selector =
                Selector::parse("div.prod-title-desc > h4.prod-title > a").unwrap();
            let title = match element.select(&title_selector).next() {
                Some(title_element) => title_element.text().next().map(|s| String::from(s.trim())),
                None => None,
            };

            // extract product description
            let description_selector =
                Selector::parse("div.prod-title-desc > p.prod-desc > a").unwrap();
            let description = match element.select(&description_selector).next() {
                Some(description_element) => description_element
                    .text()
                    .next()
                    .map(|s| String::from(s.trim())),
                None => None,
            };

            vec.push(ProductShortDetails {
                id,
                title,
                description,
                url,
            });
        }

        vec
    }

    fn get_first_page(&self) -> Url {
        self.base_url.join(self.start_url).unwrap()
    }

    fn get_next_page(&self, document: &Html) -> Option<Url> {
        let next_page_selector = Selector::parse("a.next").unwrap();

        if let Some(next_page_element) = document.select(&next_page_selector).next() {
            if let Some(next_page_value) = next_page_element.value().attr("href") {
                return self.base_url.clone().join(next_page_value).ok();
            }
        }

        None
    }

    fn get_product_details(&self, document: &Html, url: &Url) -> ProductDetails {
        let target = ScrapeTarget::Ulta;

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

        let price = None;
        let rating = None;
        let product_type = None;
        let skin_issues = None;
        let skin_type = None;
        let product_components = None;
        let id = None;

        ProductDetails {
            target,
            brand,
            title,
            description,
            price,
            rating,
            product_type,
            skin_issues,
            ingredients,
            volume,
            product_components,
            skin_type,
            id,
            how_to_use,
            product_link: url.to_string(),
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
