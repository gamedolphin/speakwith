init-db:
    mkdir -p data & sqlx database setup --source ./pkg/database/migrations/
watch-tw:
    cd pkg/frontend && tailwindcss -w -i ./templates/input.css -o ./assets/output.css

watch-rust:
    cargo watch -x 'run' -i "{**/*.html,**/*.css,**/*.jinja2,**/*.sqlite*,**/uploads/**}"
    
run:
    just watch-rust & just watch-tw
