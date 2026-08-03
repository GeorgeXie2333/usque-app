package cmd

import (
	"fmt"
	"log"
	"text/tabwriter"
	"time"

	"github.com/Diniboy1123/usque/api"
	"github.com/Diniboy1123/usque/config"
	"github.com/spf13/cobra"
)

func writeTabbedLine(writer *tabwriter.Writer, format string, args ...any) {
	if _, err := fmt.Fprintf(writer, format, args...); err != nil {
		log.Fatalf("Failed to write output: %v\n", err)
	}
}

var accountCmd = &cobra.Command{
	Use:   "account",
	Short: "Manage account and license keys",
	Long:  "Manage account information and license key operations for WARP.",
}

var accountInfoCmd = &cobra.Command{
	Use:   "info",
	Short: "Display current account information",
	Long:  "Display detailed information about the current WARP account.",
	Run: func(cmd *cobra.Command, args []string) {
		if !config.ConfigLoaded {
			log.Fatalln("Config not loaded. Please register first.")
		}

		account, err := api.GetAccount(config.AppConfig.ID, config.AppConfig.AccessToken)
		if err != nil {
			log.Fatalf("Failed to get account: %v\n", err)
		}

		writer := tabwriter.NewWriter(cmd.OutOrStdout(), 0, 0, 2, ' ', 0)
		defer func() {
			if err := writer.Flush(); err != nil {
				log.Fatalf("Failed to flush output: %v\n", err)
			}
		}()

		writeTabbedLine(writer, "Account ID:\t%s\n", account.ID)

		if len(account.AccountType) > 0 {
			writeTabbedLine(writer, "Type:\t%s\n", account.AccountType)
		}

		if created, err := time.Parse(time.RFC3339Nano, account.Created); err == nil {
			writeTabbedLine(writer, "Created:\t%s\n", created.Format(time.DateTime))
		}

		if updated, err := time.Parse(time.RFC3339Nano, account.Updated); err == nil {
			writeTabbedLine(writer, "Updated:\t%s\n", updated.Format(time.DateTime))
		}

		if managed, err := time.Parse(time.RFC3339Nano, account.Managed); err == nil {
			writeTabbedLine(writer, "Managed:\t%s\n", managed.Format(time.DateTime))
		}

		if len(account.Organization) > 0 {
			writeTabbedLine(writer, "Organization:\t%s\n", account.Organization)
		}

		if len(account.Role) > 0 {
			writeTabbedLine(writer, "Role:\t%s\n", account.Role)
		}

		if len(account.License) > 0 {
			writeTabbedLine(writer, "Licence Key:\t%s\n", account.License)
		}

		if account.PremiumData > 0 {
			writeTabbedLine(writer, "Premium Data:\t%v\n", account.PremiumData)
		}

		if account.Quota > 0 {
			writeTabbedLine(writer, "Quota:\t%v\n", account.Quota)
		}

		if account.ReferralCount > 0 {
			writeTabbedLine(writer, "Referral Count:\t%v\n", account.ReferralCount)
		}

		if account.ReferralRenewalCount > 0 {
			writeTabbedLine(writer, "Referral Renewal Countdown:\t%v\n", account.ReferralRenewalCount)
		}
	},
}

var accountDevicesCmd = &cobra.Command{
	Use:   "devices",
	Short: "List connected devices",
	Long:  "Display information about devices connected with the current WARP account.",
	Run: func(cmd *cobra.Command, args []string) {
		if !config.ConfigLoaded {
			log.Fatalln("Config not loaded. Please register first.")
		}

		devices, err := api.GetDevices(config.AppConfig.ID, config.AppConfig.AccessToken)
		if err != nil {
			log.Fatalf("Failed to get devices: %v\n", err)
			return
		}

		writer := tabwriter.NewWriter(cmd.OutOrStdout(), 0, 0, 2, ' ', 0)
		defer func() {
			if err := writer.Flush(); err != nil {
				log.Fatalf("Failed to flush output: %v\n", err)
			}
		}()

		for index, account := range *devices {
			writeTabbedLine(writer, "Device #%d:\n", index+1)
			writeTabbedLine(writer, "\tID:\t%s\n", account.ID)

			if len(account.Type) > 0 {
				writeTabbedLine(writer, "\tType:\t%s\n", account.Type)
			}

			if len(account.Model) > 0 {
				writeTabbedLine(writer, "\tModel:\t%s\n", account.Model)
			}

			if len(account.Name) > 0 {
				writeTabbedLine(writer, "\tName:\t%s\n", account.Name)
			}

			if created, err := time.Parse(time.RFC3339Nano, account.Created); err == nil {
				writeTabbedLine(writer, "\tCreated:\t%s\n", created.Format(time.DateTime))
			}

			if activated, err := time.Parse(time.RFC3339Nano, account.Activated); err == nil {
				writeTabbedLine(writer, "\tActivated:\t%s\n", activated.Format(time.DateTime))
			}

			writeTabbedLine(writer, "\tActive:\t%t\n", account.Active)

			if len(account.Role) > 0 {
				writeTabbedLine(writer, "\tRole:\t%s\n", account.Role)
			}
		}
	},
}

var accountSetCmd = &cobra.Command{
	Use:   "set <LICENSE-KEY>",
	Short: "Set WARP license key",
	Long: "Bind the current device to a WARP account by setting a license key.\n" +
		"The license key must have the following format: 'xxxxxxxx-xxxxxxxx-xxxxxxxx'.",
	Args:       cobra.MinimumNArgs(1),
	ArgAliases: []string{"licence-key"},
	Run: func(cmd *cobra.Command, args []string) {
		if !config.ConfigLoaded {
			log.Fatalln("Config not loaded. Please register first.")
			return
		}

		if len(args) < 1 {
			log.Fatalln("required license-key argument")
			return
		}

		licenceKey := args[0]

		err := api.UpdateLicenceKey(config.AppConfig.ID, config.AppConfig.AccessToken, licenceKey)
		if err != nil {
			log.Fatalf("Failed to set licence key: %v\n", err)
		}

		log.Println("Licence key successfully changed")
	},
}

var accountResetCmd = &cobra.Command{
	Use:   "reset",
	Short: "Remove WARP license key",
	Long: "Unbind the current device from the WARP account by removing the license key.\n" +
		"This will free up the license key to be used on another device.",
	Run: func(cmd *cobra.Command, args []string) {
		if !config.ConfigLoaded {
			log.Fatalln("Config not loaded. Please register first.")
		}

		err := api.DeleteLicenceKey(config.AppConfig.ID, config.AppConfig.AccessToken)
		if err != nil {
			log.Fatalf("Failed to reset licence key: %v\n", err)
		}

		log.Println("Licence key successfully removed")
	},
}

func init() {
	accountCmd.AddCommand(accountInfoCmd)
	accountCmd.AddCommand(accountSetCmd)
	accountCmd.AddCommand(accountResetCmd)
	accountCmd.AddCommand(accountDevicesCmd)
	rootCmd.AddCommand(accountCmd)
}
