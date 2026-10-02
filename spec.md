`# Frontend Application Specification

This document provides a detailed breakdown of the requirements for a frontend application to be built using Astro.js. The application's purpose is to manage and display a collection of account documents, allowing users to organize them into custom, persistent lists.

## 1. Core Functionality

The application must provide the following core features:

- **Display Accounts**: Present a list of accounts to the user in a clear, browsable format.
- **List Management**: Allow users to create and select multiple lists.
- **Add to List**: Enable users to add any account from the main display to a currently active list.
- **List View**: Display a summary of all created lists, showing their total value and item count.
- **Remove from List**: Provide the ability to remove items from a list from the list view.
- **Backend Integration**: Send the contents of a selected list to a backend API.

## 2. Data Structures

The primary data will be an array of account objects. A single account object has the following structure:

```json
{
  "_id": {
    "$oid": "67822e54900a10d40ce71722"
  },
  "Number": "3799414133",
  "Name": "GOURI KRISHNA",
  "Denomination": "2000",
  "CNumber": "IHW0045521",
  "Ref_Number": "74",
  "addedIn": ""
}
```

The application should be able to handle a dataset of hundreds of these objects efficiently.

## 3. UI Components and Layout

### 3.1. Main Account Display

- The main section will be a list or grid displaying the account documents.
- Each account should be represented as a card or list item.
- Each card must clearly show the "Name", "Number", and "Denomination".
- An "Add" button will be included on each card to add the account to the active list.

### 3.2. List Management Section

- A dedicated component (e.g., a sidebar or header element) will show a list of all existing lists.
- Users can click on any list name to set it as the active list.
- The active list must be visually highlighted (e.g., a distinct background color, border, or icon).
- The application must have a default list (e.g., "Main List") that is active on the first load.

### 3.3. List Viewing Section

- A separate, accessible section will show a summary of all lists that are not empty.
- Each list summary card must display:
  - The list name.
  - **Total Denomination**: The sum of all Denomination values for accounts in that list.
  - **Total Items**: The total number of accounts in the list.
- Clicking a list summary card should expand to show the individual account items within it.
- Each item in the expanded view must have a "Remove" button to remove it from the list.

## 4. User Interaction Flow

### 4.1. Adding an Account

1. User clicks a list name to set it as the active list. This selection persists.
2. User clicks the "Add" button on an account card.
3. The account is added to the active list, and the `addedIn` field of the account object is updated with the active list's name.
4. The state of the "Add" button on that card should update to indicate that the item has been added.

### 4.2. Removing an Account

1. User navigates to the List Viewing Section.
2. User clicks a list summary card to expand it.
3. User clicks the "Remove" button next to an account item.
4. The account is removed from the list, and the list's summary totals are automatically updated.

### 4.3. Backend Submission

- A submission button will be available (e.g., within a list's expanded view).
- When clicked, the button will initiate an HTTP POST request to the backend.
- The request body will be an array containing all account objects from that specific list.
- The application should provide feedback on the request's success or failure.

## 5. State Management and Persistence

- The application must use a reactive state management approach (e.g., a framework-specific solution or a library like Zustand).
- The entire state, including the contents of all lists and the active list selection, must be persisted in browser localStorage or a similar mechanism. This ensures the user's progress is saved across sessions.

## 6. Technology Stack

- **Frontend Framework**: Astro.js
- **Styling**: A modern CSS framework (e.g., Tailwind CSS) is recommended for rapid development.
- **State**: A reactive state management solution that can be used with Astro's islands architecture.
- **Persistence**: localStorage for client-side state persistence.

## 7. Development Notes

- The initial account data can be a static JSON import to a file within the project for development.
- The UI should be clean, responsive, and intuitive.
- Consider using Astro's component islands to handle the dynamic, interactive parts of the application while keeping the main page static for performance.

## 8. Project Structure

- The Astro.js frontend code should be located within a `frontend/` directory in the parent folder.
- The user will handle the `backend/` folder and its contents separately.`