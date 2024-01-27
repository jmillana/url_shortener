# URL Shortener with Rust + HTMX + Cloudflare Workers
# Under Construction
Utilize the power of Cloudflare Workers to implement a scalable, edge-based, and efficient URL shortener, leveraging Rust and HTMX technologies.
Blazingly slow!

## Why cloudflare?
Cloudflare offers several advantages for this project:

1. Cost-effectiveness: For a zero user application, the operational costs are essentially free, making it an economical choice for initial deployment and testing.
2. Rust Integration: Cloudflare Workers seamlessly integrate with Rust WebAssembly (wasm) through the Wrangler CLI, allowing for efficient and powerful server-side logic.
3. Scalability: Despite the current absence of users, Cloudflare Workers distribute your application globally to the edge, ensuring rapid response times worldwide.

## Architecture Overview
The URL shortener system employs the following components:

- Cloudflare KV Storage: Handles the delivery of static assets such as HTML, images, and CSS. (It's worth noting that Cloudflare Pages could be a more suitable option for managing the frontend.)

- Cloudflare D1 Database: A serverless SQL database tailored for Cloudflare Workers, providing efficient data storage and retrieval capabilities.

- Cloudflare Workers: These components manage the Rust-based server logic, handling URL shortening operations and serving requests efficiently.

## Deployment Instructions
To deploy the URL shortener to Cloudflare, follow these steps:

<b>1. Setup the KV store</b>
```bash
npx wrangler kv:namespace create <KV_NAME>
npx wrangler kv:key --namespace-id=<NAMESPACE_ID> --local --path=<path/to/file> <KEY>
```
<b>2. Setup the D1 database</b>
```bash
npx wrangler d1 create <DB_NAME>
npx wrangler d1 execute --local --file db/schema.sql <DB_NAME>
``` 
<b>3. Run Your Application Locally</b>
```bash
npx wrangler dev
```
<b>4. Deploy to Cloudflare Workers</b>
```bash
npx wrangler deploy
``` 
<b>Note:</b> Remove the `--local` flag from the commands to deploy to remote Cloudflare.

These steps ensure that your URL shortener is properly configured and deployed both locally and on the Cloudflare platform, ready to handle URL shortening tasks efficiently.

By integrating Rust, HTMX, and Cloudflare Workers, this URL shortener promises not only speed and scalability but also robustness and reliability in managing shortened URLs across the web.
