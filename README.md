# CV webpage

## Development

```
docker compose up -d

diesel migration generate <name>
diesel migration run
diesel migration revert

tailwindcss -i input.css -o static/tailwind.css --minify

cargo run
```

## Admin

```
cargo run -- set-password <username>
```

