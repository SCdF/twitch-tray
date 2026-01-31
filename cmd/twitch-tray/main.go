package main

import (
	"log"
	"os"

	"github.com/user/twitch-tray/internal/app"
)

func main() {
	// Set up logging
	log.SetFlags(log.Ldate | log.Ltime | log.Lshortfile)

	// Create and run the application
	application, err := app.New()
	if err != nil {
		log.Printf("Failed to initialize application: %v", err)
		os.Exit(1)
	}

	if err := application.Run(); err != nil {
		log.Printf("Application error: %v", err)
		os.Exit(1)
	}
}
